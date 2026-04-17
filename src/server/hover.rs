//! Hover provider for the Circom LSP.
//!
//! Shows type info, signature, and description for symbols on hover.

use tower_lsp::lsp_types::*;

use crate::circomlib_docs;
use crate::symbol::{ScopeId, SymbolKind};
use crate::symbol_table::SymbolTable;

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

fn signal_hover_markdown(name: &str, sig: &crate::symbol::SignalSymbol) -> String {
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
    format!("```circom\nsignal {direction}{name}{dims}{bus}{tags}\n```")
}

fn symbol_hover_markdown(symbol: &crate::symbol::Symbol) -> String {
    match &symbol.kind {
        SymbolKind::Template(t) => template_hover_markdown(&symbol.name, t),
        SymbolKind::Function(f) => {
            let params = f.params.join(", ");
            format!("```circom\nfunction {}({})\n```", symbol.name, params)
        }
        SymbolKind::Bus(b) => {
            let params = b.params.join(", ");
            format!("```circom\nbus {}({})\n```", symbol.name, params)
        }
        SymbolKind::Signal(sig) => signal_hover_markdown(&symbol.name, sig),
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
    Some(markdown_hover(symbol_hover_markdown(symbol)))
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
}
