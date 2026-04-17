//! Hover provider for the Circom LSP.
//!
//! Shows type info, signature, and description for symbols on hover.

use tower_lsp::lsp_types::*;

use crate::symbol::{ScopeId, SymbolKind};
use crate::symbol_table::SymbolTable;

/// Build hover information for the symbol with the given name at the given
/// scope.
pub fn hover_info(
    table: &SymbolTable,
    scope: ScopeId,
    name: &str,
    file_path: &str,
) -> Option<Hover> {
    let symbol = table.lookup_with_includes(scope, name, file_path)?;

    let contents = match &symbol.kind {
        SymbolKind::Template(t) => {
            let params = t.params.join(", ");
            let mut lines = vec![format!(
                "```circom\ntemplate {}({})\n```",
                symbol.name, params
            )];
            if t.is_parallel {
                lines.push("**parallel**".to_string());
            }
            if t.is_custom {
                lines.push("**custom**".to_string());
            }
            lines.join("\n\n")
        }
        SymbolKind::Function(f) => {
            let params = f.params.join(", ");
            format!("```circom\nfunction {}({})\n```", symbol.name, params)
        }
        SymbolKind::Bus(b) => {
            let params = b.params.join(", ");
            format!("```circom\nbus {}({})\n```", symbol.name, params)
        }
        SymbolKind::Signal(sig) => {
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
            format!(
                "```circom\nsignal {direction}{}{dims}{bus}{tags}\n```",
                symbol.name
            )
        }
        SymbolKind::Variable => {
            format!("```circom\nvar {}\n```", symbol.name)
        }
        SymbolKind::Component(comp) => {
            let tmpl = comp
                .template_name
                .as_ref()
                .map(|t| format!(": {t}"))
                .unwrap_or_default();
            format!("```circom\ncomponent {}{tmpl}\n```", symbol.name)
        }
        SymbolKind::Parameter => {
            format!("```circom\nparameter {}\n```", symbol.name)
        }
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: contents,
        }),
        range: None,
    })
}
