//! Hover provider for the Circom LSP.
//!
//! Shows type info, signature, and description for symbols on hover.

use tower_lsp::lsp_types::*;

use crate::circomlib_docs;
use crate::symbol::{ScopeId, SymbolKind};
use crate::symbol_table::SymbolTable;

/// Build a short `signal kind name[dims]` line for a bus field or signal.
fn signal_field_line(name: &str, sig: &crate::symbol::SignalSymbol) -> String {
    let direction = match sig.kind {
        crate::ast::SignalKind::Input => "input ",
        crate::ast::SignalKind::Output => "output ",
        crate::ast::SignalKind::Intermediate => "",
    };
    let dims = if sig.dimensions > 0 {
        "[]".repeat(sig.dimensions)
    } else {
        String::new()
    };
    let bus = sig
        .bus_type
        .as_ref()
        .map(|b| format!(" {b}"))
        .unwrap_or_default();
    let tags = if sig.tags.is_empty() {
        String::new()
    } else {
        format!(" {{{}}}", sig.tags.join(", "))
    };
    format!("signal {direction}{name}{bus}{dims}{tags}")
}

/// Collect the bus's field lines (signals + nested bus fields) for use in
/// hover output.
fn bus_field_lines(table: &SymbolTable, body_scope: ScopeId) -> Vec<String> {
    let scope = table.scopes.get(body_scope);
    let mut lines = Vec::new();
    for name in scope.symbol_names() {
        if let Some(ids) = scope.lookup_local(name) {
            let sym = table.get_symbol(ids[0]);
            if let SymbolKind::Signal(sig) = &sym.kind {
                lines.push(format!("    {};", signal_field_line(&sym.name, sig)));
            }
        }
    }
    lines
}

fn template_hover_markdown(name: &str, t: &crate::symbol::TemplateSymbol) -> String {
    let params = t.params.join(", ");
    let mut lines = vec![format!("```circom\ntemplate {name}({params})\n```")];
    if t.is_parallel {
        lines.push("**parallel**".to_string());
    }
    if t.is_custom {
        lines.push("**custom**".to_string());
    }
    // If a user-defined template happens to share a name with a
    // documented circomlib template, append the curated docs as well.
    if let Some(entry) = circomlib_docs::lookup(name) {
        lines.push("---".to_string());
        lines.push(entry.markdown.to_string());
    }
    lines.join("\n\n")
}

fn signal_hover_markdown(
    table: &SymbolTable,
    scope: ScopeId,
    file_path: &str,
    name: &str,
    sig: &crate::symbol::SignalSymbol,
) -> String {
    let direction = match sig.kind {
        crate::ast::SignalKind::Input => "input ",
        crate::ast::SignalKind::Output => "output ",
        crate::ast::SignalKind::Intermediate => "",
    };
    let dims = if sig.dimensions > 0 {
        "[]".repeat(sig.dimensions)
    } else {
        String::new()
    };
    let bus = sig
        .bus_type
        .as_ref()
        .map(|b| format!(" bus {b}"))
        .unwrap_or_default();
    let tags = if sig.tags.is_empty() {
        String::new()
    } else {
        format!(" {{{}}}", sig.tags.join(", "))
    };
    let head = format!("```circom\nsignal {direction}{name}{dims}{bus}{tags}\n```");

    // If this signal is bus-typed, append the bus's fields for context.
    if let Some(bus_name) = &sig.bus_type {
        if let Some(bus_sym) = table.lookup_with_includes(scope, bus_name, file_path) {
            if let SymbolKind::Bus(b) = &bus_sym.kind {
                let lines = bus_field_lines(table, b.body_scope);
                if !lines.is_empty() {
                    let params = b.params.join(", ");
                    let body = lines.join("\n");
                    return format!(
                        "{head}\n\n```circom\nbus {bus_name}({params}) {{\n{body}\n}}\n```"
                    );
                }
            }
        }
    }
    head
}

fn bus_hover_markdown(table: &SymbolTable, name: &str, b: &crate::symbol::BusSymbol) -> String {
    let params = b.params.join(", ");
    let lines = bus_field_lines(table, b.body_scope);
    if lines.is_empty() {
        format!("```circom\nbus {name}({params})\n```")
    } else {
        let body = lines.join("\n");
        format!("```circom\nbus {name}({params}) {{\n{body}\n}}\n```")
    }
}

fn symbol_hover_markdown(
    table: &SymbolTable,
    scope: ScopeId,
    file_path: &str,
    symbol: &crate::symbol::Symbol,
) -> String {
    match &symbol.kind {
        SymbolKind::Template(t) => template_hover_markdown(&symbol.name, t),
        SymbolKind::Function(f) => {
            let params = f.params.join(", ");
            format!("```circom\nfunction {}({})\n```", symbol.name, params)
        }
        SymbolKind::Bus(b) => bus_hover_markdown(table, &symbol.name, b),
        SymbolKind::Signal(sig) => {
            signal_hover_markdown(table, scope, file_path, &symbol.name, sig)
        }
        SymbolKind::Variable => format!("```circom\nvar {}\n```", symbol.name),
        SymbolKind::Component(comp) => {
            let tmpl = comp
                .template_name
                .as_ref()
                .map(|t| format!(": {t}"))
                .unwrap_or_default();
            format!("```circom\ncomponent {}{tmpl}\n```", symbol.name)
        }
        SymbolKind::Parameter => format!("```circom\nparameter {}\n```", symbol.name),
    }
}

fn markdown_hover(value: String) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: None,
    }
}

/// Build hover information for the symbol with the given name at the given
/// scope.
pub fn hover_info(
    table: &SymbolTable,
    scope: ScopeId,
    name: &str,
    file_path: &str,
) -> Option<Hover> {
    // If the cursor is on a well-known circomlib template name and the name
    // is not shadowed by a local symbol, surface the curated documentation.
    // Otherwise fall back to the in-tree definition-based hover.
    let local = table.lookup_with_includes(scope, name, file_path);
    if let Some(entry) = circomlib_docs::lookup(name) {
        let is_shadowing_template = local
            .map(|s| matches!(s.kind, SymbolKind::Template(_)))
            .unwrap_or(false);
        if !is_shadowing_template {
            return Some(markdown_hover(entry.markdown.to_string()));
        }
    }

    let symbol = local?;
    Some(markdown_hover(symbol_hover_markdown(
        table, scope, file_path, symbol,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    fn extract_markdown(hover: Hover) -> String {
        match hover.contents {
            HoverContents::Markup(m) => m.value,
            _ => String::new(),
        }
    }

    #[test]
    fn surfaces_circomlib_docs_for_known_template() {
        let src = "";
        let (ast, _) = parser::parse(src);
        let mut table = SymbolTable::new();
        table.index_file("main.circom", &ast);
        let scope = table.file_scope("main.circom").unwrap();

        let hover = hover_info(&table, scope, "Num2Bits", "main.circom")
            .expect("Num2Bits hover should exist");
        let md = extract_markdown(hover);
        assert!(md.contains("Num2Bits"));
        assert!(md.contains("Params"));
    }

    #[test]
    fn local_template_overrides_circomlib_docs_but_appends() {
        // A user-defined template named `IsZero` should still be the
        // primary hover target, with circomlib docs appended.
        let src = r#"
            template IsZero() {
                signal input in;
                signal output out;
                out <== 0;
            }
        "#;
        let (ast, errors) = parser::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut table = SymbolTable::new();
        table.index_file("main.circom", &ast);
        let scope = table.file_scope("main.circom").unwrap();

        let hover = hover_info(&table, scope, "IsZero", "main.circom").unwrap();
        let md = extract_markdown(hover);
        assert!(md.contains("template IsZero"));
        assert!(md.contains("iff the input equals 0"));
    }

    #[test]
    fn unknown_symbol_returns_none() {
        let src = "";
        let (ast, _) = parser::parse(src);
        let mut table = SymbolTable::new();
        table.index_file("main.circom", &ast);
        let scope = table.file_scope("main.circom").unwrap();

        assert!(hover_info(&table, scope, "NotARealSymbol", "main.circom").is_none());
    }

    // ── Bus hover (#46) ──────────────────────────────────────────

    #[test]
    fn bus_hover_includes_field_signatures() {
        let src = r#"
            pragma circom 2.2.0;
            bus Point() {
                signal x;
                signal y;
            }
        "#;
        let (ast, _) = parser::parse(src);
        let mut table = SymbolTable::new();
        table.index_file("main.circom", &ast);
        let scope = table.file_scope("main.circom").unwrap();
        let hover = hover_info(&table, scope, "Point", "main.circom").unwrap();
        let md = extract_markdown(hover);
        assert!(md.contains("bus Point()"));
        assert!(md.contains("signal x"));
        assert!(md.contains("signal y"));
    }

    #[test]
    fn bus_signal_hover_shows_bus_body() {
        let src = r#"
            pragma circom 2.2.0;
            bus Point() {
                signal x;
                signal y;
            }
            template T() {
                signal input Point() p;
            }
        "#;
        let (ast, _) = parser::parse(src);
        let mut table = SymbolTable::new();
        table.index_file("main.circom", &ast);
        // Hover on 'p' requires looking up from the template body scope.
        use crate::symbol::SymbolKind;
        let file_scope = table.file_scope("main.circom").unwrap();
        let t_sym = table.lookup(file_scope, "T").unwrap();
        let body = match &t_sym.kind {
            SymbolKind::Template(t) => t.body_scope,
            _ => panic!("expected template"),
        };
        let hover = hover_info(&table, body, "p", "main.circom").unwrap();
        let md = extract_markdown(hover);
        assert!(md.contains("signal input p bus Point"), "got: {md}");
        assert!(md.contains("bus Point()"), "got: {md}");
        assert!(md.contains("signal x"), "got: {md}");
        assert!(md.contains("signal y"), "got: {md}");
    }
}
