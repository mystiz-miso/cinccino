//! Rename provider for `textDocument/rename`.
//!
//! Collects all references to the symbol at the cursor across every open
//! document, and returns a [`WorkspaceEdit`] containing a [`TextEdit`] for
//! every occurrence.
//!
//! A rename is rejected (returns an error) if the new name:
//!
//! * is not a valid Circom identifier, or
//! * would collide with an existing symbol in the same scope as the target
//!   (i.e. renaming to `x` where another `x` already exists in the scope).

use std::collections::HashMap;

use tower_lsp::jsonrpc::Error as JsonRpcError;
use tower_lsp::lsp_types::{Position, Range, TextEdit, Url, WorkspaceEdit};

use crate::span::LineIndex;
use crate::symbol::{ScopeId, SymbolKind};
use crate::symbol_table::SymbolTable;

/// Check that `name` is a valid Circom identifier.
pub fn is_valid_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let bytes = name.as_bytes();
    let first = bytes[0];
    if !first.is_ascii_alphabetic() && first != b'_' {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Return `true` if `new_name` would collide with an existing symbol in the
/// target scope (other than the symbol being renamed).
pub fn would_conflict(
    table: &SymbolTable,
    scope: ScopeId,
    new_name: &str,
    rename_target_file: &str,
    rename_target_offset: usize,
) -> bool {
    // A collision exists when another symbol with the same name lives in the
    // same scope (at a different location).
    if let Some(ids) = table.scopes.lookup_local(scope, new_name) {
        for id in ids {
            let sym = table.get_symbol(*id);
            let is_target =
                sym.file == rename_target_file && sym.span.start == rename_target_offset;
            if !is_target {
                return true;
            }
        }
    }
    false
}

/// Compute the [`WorkspaceEdit`] for renaming the symbol `target_name`
/// (defined at `target_symbol_file` / `target_symbol_offset`) to `new_name`
/// across every document in `documents`.
///
/// The caller must have already:
/// * resolved the cursor to the target symbol,
/// * verified the new name (via [`is_valid_identifier`]) and absence of
///   conflicts (via [`would_conflict`]).
pub fn build_workspace_edit(
    target_name: &str,
    new_name: &str,
    target_file: &str,
    target_offset: usize,
    documents: &[(Url, String)],
) -> WorkspaceEdit {
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

    for (uri, text) in documents {
        let edits = find_identifier_edits(text, target_name, new_name);
        if !edits.is_empty() {
            changes.insert(uri.clone(), edits);
        }
    }

    // Suppress unused warnings on the caller-resolved target location; we keep
    // the parameters in the signature so the caller interface matches the use
    // case (a future enhancement could filter by span to rename only the
    // exact referenced symbol rather than every text occurrence).
    let _ = target_file;
    let _ = target_offset;

    WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    }
}

/// Scan `text` for every whole-word occurrence of `name` and produce a
/// [`TextEdit`] that replaces each with `new_name`.
///
/// Skips matches that fall inside string literals, line comments, and
/// block comments — otherwise renaming `foo` would corrupt any string
/// or comment that happens to contain the substring.
fn find_identifier_edits(text: &str, name: &str, new_name: &str) -> Vec<TextEdit> {
    let line_index = LineIndex::new(text);
    let bytes = text.as_bytes();
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len();

    let skip_ranges = scan_skip_ranges(text);
    let in_skip_range = |offset: usize| -> bool {
        skip_ranges
            .iter()
            .any(|(start, end)| offset >= *start && offset < *end)
    };

    let mut edits = Vec::new();
    let mut pos = 0;

    while pos + name_len <= bytes.len() {
        match text[pos..].find(name) {
            Some(found) => {
                let abs = pos + found;
                let before_ok = abs == 0 || !is_ident_byte(bytes[abs - 1]);
                let after_ok =
                    abs + name_len >= bytes.len() || !is_ident_byte(bytes[abs + name_len]);

                if before_ok && after_ok && !in_skip_range(abs) {
                    if let (Some(start_lc), Some(end_lc)) = (
                        line_index.line_col(abs),
                        line_index.line_col(abs + name_len),
                    ) {
                        edits.push(TextEdit {
                            range: Range {
                                start: Position {
                                    line: start_lc.line,
                                    character: start_lc.col,
                                },
                                end: Position {
                                    line: end_lc.line,
                                    character: end_lc.col,
                                },
                            },
                            new_text: new_name.to_string(),
                        });
                    }
                }
                pos = abs + name_len;
            }
            None => break,
        }
    }

    edits
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Scan `text` and return byte ranges that should be skipped when
/// matching identifiers: string literals, line comments, block comments.
fn scan_skip_ranges(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < bytes.len() {
                    i += 1;
                }
                ranges.push((start, i));
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                let start = i;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                ranges.push((start, i));
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                let start = i;
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                if i + 1 < bytes.len() {
                    i += 2;
                }
                ranges.push((start, i));
            }
            _ => i += 1,
        }
    }

    ranges
}

/// Build the error returned to the client when the requested new name is
/// invalid or would cause a conflict.
pub fn invalid_rename_error(message: &str) -> JsonRpcError {
    JsonRpcError {
        code: tower_lsp::jsonrpc::ErrorCode::InvalidParams,
        message: message.to_string().into(),
        data: None,
    }
}

/// Get the scope that defines `symbol` — this is the scope we must
/// check for conflicts against when renaming.
///
/// For most symbols this is `symbol.scope` itself. For templates, functions,
/// and buses, callers that want to rename the top-level definition pass the
/// file scope.
pub fn defining_scope(table: &SymbolTable, file_path: &str, name: &str) -> Option<ScopeId> {
    let file_scope = table.file_scope(file_path)?;
    let sym = table.lookup_with_includes(file_scope, name, file_path)?;
    Some(sym.scope)
}

/// Whether a symbol kind is renameable from the LSP (we skip parameters and
/// anything else that is purely local when needed, but for now all symbol
/// kinds are renameable).
pub fn is_renameable(kind: &SymbolKind) -> bool {
    // All user-declared symbols are renameable. We could further restrict
    // this later (e.g. to avoid renaming Circom built-ins if they were ever
    // represented in the symbol table).
    !matches!(kind, SymbolKind::Parameter if false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_identifier_accepts_ascii_names() {
        assert!(is_valid_identifier("foo"));
        assert!(is_valid_identifier("_foo"));
        assert!(is_valid_identifier("foo123"));
        assert!(is_valid_identifier("Foo_Bar"));
    }

    #[test]
    fn valid_identifier_rejects_bad_names() {
        assert!(!is_valid_identifier(""));
        assert!(!is_valid_identifier("1foo"));
        assert!(!is_valid_identifier("foo-bar"));
        assert!(!is_valid_identifier("foo bar"));
    }

    #[test]
    fn find_edits_replaces_whole_words_only() {
        let text = "template Foo() { Foo; Foobar; _Foo; Foo_; }";
        let edits = find_identifier_edits(text, "Foo", "Bar");
        // Expected matches: "Foo" at col 9, col 17 — "Foobar" and "_Foo" and
        // "Foo_" are not whole-word matches.
        assert_eq!(edits.len(), 2);
    }

    #[test]
    fn find_edits_skips_matches_inside_strings() {
        let text = r#"template Foo() { log("Foo is Foo"); Foo; }"#;
        let edits = find_identifier_edits(text, "Foo", "Bar");
        // Only the outer "Foo" identifiers should match — the two inside
        // the "Foo is Foo" string literal must be skipped.
        assert_eq!(edits.len(), 2);
    }

    #[test]
    fn find_edits_skips_matches_inside_line_comments() {
        let text = "template Foo() { // Foo in a comment\n  Foo;\n}";
        let edits = find_identifier_edits(text, "Foo", "Bar");
        // "Foo" at col 9 and inside the body — the comment reference must be skipped.
        assert_eq!(edits.len(), 2);
    }

    #[test]
    fn find_edits_skips_matches_inside_block_comments() {
        let text = "/* Foo everywhere */\ntemplate Foo() { Foo; }";
        let edits = find_identifier_edits(text, "Foo", "Bar");
        assert_eq!(edits.len(), 2);
    }

    #[test]
    fn build_workspace_edit_collects_across_files() {
        let doc_a: (Url, String) = (
            Url::parse("file:///a.circom").unwrap(),
            "template Foo() {}\ncomponent c = Foo();\n".to_string(),
        );
        let doc_b: (Url, String) = (
            Url::parse("file:///b.circom").unwrap(),
            "include \"a.circom\";\ncomponent d = Foo();\n".to_string(),
        );
        let docs = [doc_a, doc_b];

        let edit = build_workspace_edit("Foo", "Bar", "file:///a.circom", 9, &docs);
        let changes = edit.changes.unwrap();
        assert_eq!(changes.len(), 2);
        assert!(
            changes
                .get(&Url::parse("file:///a.circom").unwrap())
                .unwrap()
                .len()
                >= 2
        );
        assert!(!changes
            .get(&Url::parse("file:///b.circom").unwrap())
            .unwrap()
            .is_empty());
    }
}
