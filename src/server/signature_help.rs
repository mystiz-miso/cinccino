//! Signature help for `textDocument/signatureHelp`.
//!
//! Provides parameter information when the cursor is inside a function call,
//! template instantiation, or built-in call (`log`, `assert`, `NumBits`).

use tower_lsp::lsp_types::{
    ParameterInformation, ParameterLabel, SignatureHelp, SignatureInformation,
};

use crate::symbol::SymbolKind;
use crate::symbol_table::SymbolTable;

/// Result of finding a call site at a cursor position.
#[derive(Debug, Clone, PartialEq)]
pub struct CallSite {
    /// The name of the function/template being called.
    pub name: String,
    /// The 0-based index of the active parameter.
    pub active_param: u32,
}

/// Compute signature help for a cursor at byte `offset` in `source`.
///
/// Returns `None` if the cursor is not inside a call expression.
pub fn signature_help(
    source: &str,
    offset: usize,
    symbol_table: &SymbolTable,
    file_path: &str,
) -> Option<SignatureHelp> {
    let call_site = find_call_site(source, offset)?;

    // Look up the symbol to get parameter names.
    let file_scope = symbol_table.file_scope(file_path)?;
    let symbol = symbol_table.lookup_with_includes(file_scope, &call_site.name, file_path)?;

    let (label, params) = match &symbol.kind {
        SymbolKind::Template(t) => build_signature(&call_site.name, &t.params),
        SymbolKind::Function(f) => build_signature(&call_site.name, &f.params),
        _ => return None,
    };

    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label,
            documentation: None,
            parameters: Some(params),
            active_parameter: Some(call_site.active_param),
        }],
        active_signature: Some(0),
        active_parameter: Some(call_site.active_param),
    })
}

/// Compute signature help for built-in functions (log, assert, NumBits).
///
/// These don't exist in the symbol table, so we handle them separately.
pub fn builtin_signature_help(name: &str, active_param: u32) -> Option<SignatureHelp> {
    let (label, params) = match name {
        "log" => (
            "log(expr, ...)".to_string(),
            vec![ParameterInformation {
                label: ParameterLabel::Simple("expr, ...".to_string()),
                documentation: None,
            }],
        ),
        "assert" => (
            "assert(condition)".to_string(),
            vec![ParameterInformation {
                label: ParameterLabel::Simple("condition".to_string()),
                documentation: None,
            }],
        ),
        "NumBits" => (
            "NumBits(n)".to_string(),
            vec![ParameterInformation {
                label: ParameterLabel::Simple("n".to_string()),
                documentation: None,
            }],
        ),
        _ => return None,
    };

    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label,
            documentation: None,
            parameters: Some(params),
            active_parameter: Some(active_param),
        }],
        active_signature: Some(0),
        active_parameter: Some(active_param),
    })
}

/// Build a signature label and parameter list from a name and param names.
fn build_signature(name: &str, param_names: &[String]) -> (String, Vec<ParameterInformation>) {
    let params_str = param_names.join(", ");
    let label = format!("{name}({params_str})");
    let params = param_names
        .iter()
        .map(|p| ParameterInformation {
            label: ParameterLabel::Simple(p.clone()),
            documentation: None,
        })
        .collect();
    (label, params)
}

/// Find the call site (function name + active parameter index) at a byte offset.
///
/// Scans backward from `offset` through the source text to find a `(` that
/// starts a call, counting `,` at the same nesting depth to determine the
/// active parameter.
///
/// **Known limitation**: this scan does not skip `//` or `/* ... */` comments
/// or string literals (`"..."`). A comma or parenthesis inside a comment or
/// string will be incorrectly counted, producing a wrong `active_param` or a
/// spurious/missing call site. Circom supports both comment styles.
pub fn find_call_site(source: &str, offset: usize) -> Option<CallSite> {
    // Clamp offset to source length.
    let offset = offset.min(source.len());

    // Scan backward to find the opening `(` of the call, tracking nesting.
    let bytes = source.as_bytes();
    let mut depth: i32 = 0;
    let mut comma_count: u32 = 0;
    let mut pos = offset;

    // We need to handle the case where cursor is right after `(`
    // Walk backward through the text
    while pos > 0 {
        pos -= 1;
        match bytes[pos] {
            b')' => depth += 1,
            b'(' => {
                if depth == 0 {
                    // Found the opening paren. Extract the callee name.
                    let name = extract_callee_name(source, pos)?;
                    return Some(CallSite {
                        name,
                        active_param: comma_count,
                    });
                }
                depth -= 1;
            }
            b',' if depth == 0 => {
                comma_count += 1;
            }
            // Stop scanning at statement boundaries
            b';' | b'{' | b'}' if depth == 0 => return None,
            _ => {}
        }
    }

    None
}

/// Extract the callee name just before the opening `(` at position `paren_pos`.
///
/// Skips whitespace, then reads a contiguous identifier.
fn extract_callee_name(source: &str, paren_pos: usize) -> Option<String> {
    let bytes = source.as_bytes();
    let mut end = paren_pos;

    // Skip whitespace before `(`
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    if end == 0 {
        return None;
    }

    // Read identifier characters backward
    let mut start = end;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }

    if start == end {
        return None;
    }

    let name = &source[start..end];
    // Verify it's a valid identifier (starts with letter or _)
    let first = name.as_bytes()[0];
    if !first.is_ascii_alphabetic() && first != b'_' {
        return None;
    }

    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    /// Helper: find call site at the position marked by `|`.
    fn call_site_at(src_with_cursor: &str) -> Option<CallSite> {
        let cursor_pos = src_with_cursor.find('|').expect("source must contain '|'");
        let source: String = src_with_cursor.chars().filter(|&c| c != '|').collect();
        find_call_site(&source, cursor_pos)
    }

    #[test]
    fn template_instantiation_first_param() {
        let site = call_site_at("template T() { component c = Poseidon(|); }");
        assert_eq!(
            site,
            Some(CallSite {
                name: "Poseidon".to_string(),
                active_param: 0,
            })
        );
    }

    #[test]
    fn function_call_first_param() {
        let site = call_site_at("function f() { return nbits(|); }");
        assert_eq!(
            site,
            Some(CallSite {
                name: "nbits".to_string(),
                active_param: 0,
            })
        );
    }

    #[test]
    fn active_param_advances_on_comma() {
        let site = call_site_at("template T() { component c = Foo(1, |); }");
        assert_eq!(
            site,
            Some(CallSite {
                name: "Foo".to_string(),
                active_param: 1,
            })
        );
    }

    #[test]
    fn active_param_third() {
        let site = call_site_at("template T() { component c = Foo(1, 2, |); }");
        assert_eq!(
            site,
            Some(CallSite {
                name: "Foo".to_string(),
                active_param: 2,
            })
        );
    }

    #[test]
    fn builtin_log() {
        let site = call_site_at("template T() { log(|); }");
        assert_eq!(
            site,
            Some(CallSite {
                name: "log".to_string(),
                active_param: 0,
            })
        );
    }

    #[test]
    fn builtin_assert() {
        let site = call_site_at("template T() { assert(|); }");
        assert_eq!(
            site,
            Some(CallSite {
                name: "assert".to_string(),
                active_param: 0,
            })
        );
    }

    #[test]
    fn no_signature_outside_call() {
        let site = call_site_at("template T() { var x = |1; }");
        assert_eq!(site, None);
    }

    #[test]
    fn no_signature_after_closing_paren() {
        let site = call_site_at("template T() { var x = foo(1)|; }");
        assert_eq!(site, None);
    }

    #[test]
    fn nested_call_inner() {
        // Cursor inside inner call `bar(`
        let site = call_site_at("template T() { var x = foo(bar(|)); }");
        assert_eq!(
            site,
            Some(CallSite {
                name: "bar".to_string(),
                active_param: 0,
            })
        );
    }

    #[test]
    fn nested_call_outer_second_param() {
        // Cursor after inner call in outer call's second param position
        let site = call_site_at("template T() { var x = foo(bar(1), |); }");
        assert_eq!(
            site,
            Some(CallSite {
                name: "foo".to_string(),
                active_param: 1,
            })
        );
    }

    #[test]
    fn log_multiple_args() {
        let site = call_site_at("template T() { log(x, |); }");
        assert_eq!(
            site,
            Some(CallSite {
                name: "log".to_string(),
                active_param: 1,
            })
        );
    }

    #[test]
    fn build_signature_formats_correctly() {
        let (label, params) = build_signature("Poseidon", &["nInputs".to_string()]);
        assert_eq!(label, "Poseidon(nInputs)");
        assert_eq!(params.len(), 1);
        assert_eq!(
            params[0].label,
            ParameterLabel::Simple("nInputs".to_string())
        );
    }

    #[test]
    fn build_signature_multiple_params() {
        let (label, params) = build_signature(
            "MyFunc",
            &["a".to_string(), "b".to_string(), "c".to_string()],
        );
        assert_eq!(label, "MyFunc(a, b, c)");
        assert_eq!(params.len(), 3);
    }

    #[test]
    fn builtin_signature_help_log() {
        let help = builtin_signature_help("log", 0).unwrap();
        assert_eq!(help.signatures[0].label, "log(expr, ...)");
        assert_eq!(help.active_parameter, Some(0));
    }

    #[test]
    fn builtin_signature_help_assert() {
        let help = builtin_signature_help("assert", 0).unwrap();
        assert_eq!(help.signatures[0].label, "assert(condition)");
    }

    #[test]
    fn builtin_signature_help_numbits() {
        let help = builtin_signature_help("NumBits", 0).unwrap();
        assert_eq!(help.signatures[0].label, "NumBits(n)");
        assert_eq!(help.active_parameter, Some(0));
    }

    #[test]
    fn builtin_numbits_call_site() {
        let site = call_site_at("template T() { var x = NumBits(|); }");
        assert_eq!(
            site,
            Some(CallSite {
                name: "NumBits".to_string(),
                active_param: 0,
            })
        );
    }

    #[test]
    fn builtin_signature_help_unknown() {
        assert!(builtin_signature_help("unknown", 0).is_none());
    }

    #[test]
    fn signature_help_with_symbol_table() {
        let source = r#"
function nbits(n) {
    return n;
}
template T() {
    var x = nbits(1);
}
"#;
        let (ast, _) = parser::parse(source);
        let mut st = SymbolTable::new();
        st.index_file("test.circom", &ast);

        // Cursor inside nbits(|1)
        // "nbits(" is at some offset; let's find it
        let call_pos = source.find("nbits(1").unwrap() + 6; // right after '('
        let help = signature_help(source, call_pos, &st, "test.circom");
        let help = help.unwrap();
        assert_eq!(help.signatures[0].label, "nbits(n)");
        assert_eq!(help.active_parameter, Some(0));
    }

    #[test]
    fn signature_help_template_with_params() {
        let source = r#"
template Poseidon(nInputs) {
    signal input in;
}
template T() {
    component c = Poseidon(2);
}
"#;
        let (ast, _) = parser::parse(source);
        let mut st = SymbolTable::new();
        st.index_file("test.circom", &ast);

        let call_pos = source.find("Poseidon(2").unwrap() + 9; // after '('
        let help = signature_help(source, call_pos, &st, "test.circom");
        let help = help.unwrap();
        assert_eq!(help.signatures[0].label, "Poseidon(nInputs)");
        assert_eq!(help.active_parameter, Some(0));
    }

    #[test]
    fn no_signature_help_outside_call() {
        let source = "template T() { var x = 1; }";
        let (ast, _) = parser::parse(source);
        let mut st = SymbolTable::new();
        st.index_file("test.circom", &ast);

        // Cursor at `1`
        let pos = source.find("1;").unwrap();
        let help = signature_help(source, pos, &st, "test.circom");
        assert!(help.is_none());
    }
}
