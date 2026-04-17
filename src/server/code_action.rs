//! Code action / quick-fix provider for `textDocument/codeAction`.
//!
//! Provides quick fixes for common Circom diagnostics:
//!
//! * **Declare missing signal/variable** — inserts a `signal` or `var`
//!   declaration at the start of the enclosing template/function body when
//!   an undeclared-symbol diagnostic is reported.
//! * **Change `<--` to `<==`** — swaps an unsafe signal assignment for a
//!   constraining assignment.

use std::collections::HashMap;

use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic, DiagnosticSeverity, Position,
    Range, TextEdit, Url, WorkspaceEdit,
};

use crate::span::LineIndex;

/// Build every applicable code action for the diagnostics in `range`
/// reported against `uri` / `text`.
pub fn code_actions(uri: &Url, text: &str, diagnostics: &[Diagnostic]) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    for diag in diagnostics {
        if let Some(action) = declare_missing_symbol_action(uri, text, diag) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
        if let Some(action) = change_unsafe_assign_action(uri, text, diag) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
    }

    actions
}

/// If `diag` reports an undeclared symbol, build an action that declares it
/// as a `signal` (inside a template) or `var` (inside a function) at the
/// start of the enclosing body.
fn declare_missing_symbol_action(uri: &Url, text: &str, diag: &Diagnostic) -> Option<CodeAction> {
    // Match messages produced by `check_undeclared`:
    //     "undeclared symbol '<name>'"
    let name = extract_quoted_name(&diag.message, "undeclared symbol")?;

    // Determine the enclosing block context and insertion point.
    let diag_offset = offset_of_position(text, diag.range.start)?;
    let context = find_enclosing_context(text, diag_offset)?;

    let decl_text = match context.kind {
        EnclosingKind::Template => format!("    signal {name};\n"),
        EnclosingKind::Function => format!("    var {name};\n"),
    };

    // Insert at the line after the opening `{`.
    let line_index = LineIndex::new(text);
    let lc = line_index.line_col(context.insert_offset)?;
    let insert_pos = Position {
        line: lc.line,
        character: lc.col,
    };

    let title = match context.kind {
        EnclosingKind::Template => format!("Declare missing signal '{name}'"),
        EnclosingKind::Function => format!("Declare missing variable '{name}'"),
    };

    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: decl_text,
        }],
    );

    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    })
}

/// If `diag` is an unsafe-signal-assignment warning (severity WARNING and the
/// source range contains the `<--` operator), return an action that replaces
/// `<--` with `<==`.
fn change_unsafe_assign_action(uri: &Url, text: &str, diag: &Diagnostic) -> Option<CodeAction> {
    // Only WARNING diagnostics can be unsafe assignments; filter early to
    // avoid a text scan on every diagnostic.
    if diag.severity != Some(DiagnosticSeverity::WARNING) {
        return None;
    }

    // Search for `<--` inside the diagnostic's range.
    let start = offset_of_position(text, diag.range.start)?;
    let end = offset_of_position(text, diag.range.end)?.max(start);
    let end = end.min(text.len());
    let haystack = &text[start..end];
    let rel = haystack.find("<--")?;
    let abs = start + rel;

    let line_index = LineIndex::new(text);
    let lc_start = line_index.line_col(abs)?;
    let lc_end = line_index.line_col(abs + 3)?;

    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range {
                start: Position {
                    line: lc_start.line,
                    character: lc_start.col,
                },
                end: Position {
                    line: lc_end.line,
                    character: lc_end.col,
                },
            },
            new_text: "<==".to_string(),
        }],
    );

    Some(CodeAction {
        title: "Change '<--' to '<=='".to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    })
}

/// Extract a quoted identifier from a message like `"undeclared symbol 'foo'"`.
fn extract_quoted_name(message: &str, prefix: &str) -> Option<String> {
    if !message.contains(prefix) {
        return None;
    }
    let start = message.find('\'')?;
    let rest = &message[start + 1..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// Kind of body that encloses an offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnclosingKind {
    Template,
    Function,
}

#[derive(Debug)]
struct EnclosingContext {
    kind: EnclosingKind,
    /// Byte offset where a declaration should be inserted (start of the line
    /// after the opening `{`).
    insert_offset: usize,
}

/// Find the template/function body that encloses `offset` by scanning
/// backward through the source text.
///
/// This is a lightweight lexical scan — it does not parse the document.
/// It locates the most recent `template` or `function` keyword whose body
/// brace pair contains `offset`.
/// Walk backward from `offset` to find the unmatched `{` that opens the
/// enclosing body. Returns the byte offset of that brace.
fn find_enclosing_open_brace(bytes: &[u8], offset: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut pos = offset;
    while pos > 0 {
        pos -= 1;
        match bytes[pos] {
            b'}' => depth += 1,
            b'{' => {
                if depth == 0 {
                    return Some(pos);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Skip back across a balanced `(...)` ending at position `p`. `p` is the
/// index one past the closing `)`.
fn skip_back_over_parens(bytes: &[u8], mut p: usize) -> usize {
    if p > 0 && bytes[p - 1] == b')' {
        let mut pdepth: i32 = 0;
        while p > 0 {
            p -= 1;
            match bytes[p] {
                b')' => pdepth += 1,
                b'(' => {
                    pdepth -= 1;
                    if pdepth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    p
}

fn skip_ws_back(bytes: &[u8], mut p: usize) -> usize {
    while p > 0 && bytes[p - 1].is_ascii_whitespace() {
        p -= 1;
    }
    p
}

fn skip_ident_back(bytes: &[u8], mut p: usize) -> usize {
    while p > 0 && (bytes[p - 1].is_ascii_alphanumeric() || bytes[p - 1] == b'_') {
        p -= 1;
    }
    p
}

fn find_enclosing_context(text: &str, offset: usize) -> Option<EnclosingContext> {
    let bytes = text.as_bytes();
    let offset = offset.min(bytes.len());

    let brace = find_enclosing_open_brace(bytes, offset)?;

    // From the brace, walk backward skipping whitespace and a possible
    // `(...)` parameter list, then read the keyword.
    let mut p = brace;
    p = skip_ws_back(bytes, p);
    p = skip_back_over_parens(bytes, p);
    p = skip_ws_back(bytes, p);
    p = skip_ident_back(bytes, p);

    // Skip whitespace + optional `custom`/`parallel` modifiers.
    loop {
        p = skip_ws_back(bytes, p);
        let modifier_start = p;
        p = skip_ident_back(bytes, p);
        if p == modifier_start {
            break;
        }
        let word = &text[p..modifier_start];
        if word == "custom" || word == "parallel" {
            continue;
        }
        let kind = match word {
            "template" => EnclosingKind::Template,
            "function" => EnclosingKind::Function,
            _ => return None,
        };
        let insert_offset = next_line_start(text, brace + 1);
        return Some(EnclosingContext {
            kind,
            insert_offset,
        });
    }

    None
}

/// Byte offset of the start of the line after `offset`. If `offset` is
/// already at a newline we return the byte just after it.
fn next_line_start(text: &str, offset: usize) -> usize {
    let bytes = text.as_bytes();
    let mut p = offset;
    while p < bytes.len() && bytes[p] != b'\n' {
        p += 1;
    }
    if p < bytes.len() {
        p + 1
    } else {
        p
    }
}

/// Convert an LSP position to a byte offset using UTF-16-as-bytes semantics
/// (Circom source is ASCII so this is safe).
fn offset_of_position(text: &str, pos: Position) -> Option<usize> {
    let line_index = LineIndex::new(text);
    line_index.offset(pos.line as usize, pos.character as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_quoted_name() {
        assert_eq!(
            extract_quoted_name("undeclared symbol 'foo'", "undeclared symbol"),
            Some("foo".to_string())
        );
        assert_eq!(
            extract_quoted_name("other message", "undeclared symbol"),
            None
        );
    }

    #[test]
    fn finds_template_context() {
        let text = "template Foo() {\n    bar;\n}\n";
        // Position of `bar` (line 1, col 4)
        let offset = text.find("bar").unwrap();
        let ctx = find_enclosing_context(text, offset).unwrap();
        assert_eq!(ctx.kind, EnclosingKind::Template);
    }

    #[test]
    fn finds_function_context() {
        let text = "function foo() {\n    x = y;\n}\n";
        let offset = text.find("x = y").unwrap();
        let ctx = find_enclosing_context(text, offset).unwrap();
        assert_eq!(ctx.kind, EnclosingKind::Function);
    }

    #[test]
    fn declares_missing_signal() {
        let text = "template Foo() {\n    c <== a;\n}\n";
        let uri = Url::parse("file:///test.circom").unwrap();
        let diag = Diagnostic {
            range: Range {
                start: Position {
                    line: 1,
                    character: 9,
                },
                end: Position {
                    line: 1,
                    character: 10,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            message: "undeclared symbol 'a'".to_string(),
            source: Some("cinccino".to_string()),
            ..Default::default()
        };
        let action = declare_missing_symbol_action(&uri, text, &diag).unwrap();
        assert!(action.title.contains("signal"));
        assert!(action.title.contains('a'));
        let edit = action.edit.unwrap();
        let changes = edit.changes.unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(edits[0].new_text.contains("signal a;"));
    }

    #[test]
    fn declares_missing_variable_in_function() {
        let text = "function f() {\n    return x;\n}\n";
        let uri = Url::parse("file:///test.circom").unwrap();
        let diag = Diagnostic {
            range: Range {
                start: Position {
                    line: 1,
                    character: 11,
                },
                end: Position {
                    line: 1,
                    character: 12,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            message: "undeclared symbol 'x'".to_string(),
            source: Some("cinccino".to_string()),
            ..Default::default()
        };
        let action = declare_missing_symbol_action(&uri, text, &diag).unwrap();
        assert!(action.title.contains("variable"));
        let edit = action.edit.unwrap();
        let edits = edit.changes.unwrap().get(&uri).unwrap().clone();
        assert!(edits[0].new_text.contains("var x;"));
    }

    #[test]
    fn changes_unsafe_assign() {
        let text = "template T() {\n    b <-- a;\n}\n";
        let uri = Url::parse("file:///test.circom").unwrap();
        let diag = Diagnostic {
            range: Range {
                start: Position {
                    line: 1,
                    character: 4,
                },
                end: Position {
                    line: 1,
                    character: 11,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            message: "unsafe signal assignment".to_string(),
            source: Some("cinccino".to_string()),
            ..Default::default()
        };
        let action = change_unsafe_assign_action(&uri, text, &diag).unwrap();
        assert!(action.title.contains("<=="));
        let edit = action.edit.unwrap();
        let edits = edit.changes.unwrap().get(&uri).unwrap().clone();
        assert_eq!(edits[0].new_text, "<==");
    }

    #[test]
    fn ignores_non_warning_for_unsafe_assign() {
        let text = "template T() {\n    b <-- a;\n}\n";
        let uri = Url::parse("file:///test.circom").unwrap();
        let diag = Diagnostic {
            range: Range {
                start: Position {
                    line: 1,
                    character: 4,
                },
                end: Position {
                    line: 1,
                    character: 11,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            message: "some error".to_string(),
            source: Some("cinccino".to_string()),
            ..Default::default()
        };
        assert!(change_unsafe_assign_action(&uri, text, &diag).is_none());
    }

    #[test]
    fn code_actions_empty_for_no_matching_diagnostics() {
        let text = "template T() { signal input a; }";
        let uri = Url::parse("file:///test.circom").unwrap();
        let actions = code_actions(&uri, text, &[]);
        assert!(actions.is_empty());
    }
}
