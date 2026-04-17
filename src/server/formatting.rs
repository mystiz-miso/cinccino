//! LSP `textDocument/formatting` handler.
//!
//! Takes the full buffer text and produces a single [`TextEdit`] that
//! replaces the whole document with its formatted version. Parse-error
//! buffers are returned unchanged (no edit) so that we never clobber
//! a user's partially-edited source.

use tower_lsp::lsp_types::{FormattingOptions, FormattingProperty, Position, Range, TextEdit};

use crate::formatter::{format_source, FormatConfig};
use crate::span::LineIndex;

/// Compute a single full-document [`TextEdit`] that replaces `source`
/// with its formatted rendering.
///
/// Returns `None` when the source cannot be parsed (so the buffer is
/// left untouched) or when formatting produced the same text (no-op).
pub fn format_document(source: &str, options: &FormattingOptions) -> Option<Vec<TextEdit>> {
    let cfg = config_from_options(options);
    let formatted = format_source(source, &cfg).ok()?;
    if formatted == source {
        return Some(Vec::new());
    }

    let line_index = LineIndex::new(source);
    // Compute end line/col from the source length.
    let end_pos = end_position(&line_index, source);

    Some(vec![TextEdit {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: end_pos,
        },
        new_text: formatted,
    }])
}

/// Derive a [`FormatConfig`] from the client's [`FormattingOptions`].
///
/// The LSP provides tab-size and insertSpaces but no max-line-length.
/// A custom client-scoped option `maxLineLength` (under
/// `FormattingOptions::properties`) is honoured when present so
/// editors can surface it through settings.
fn config_from_options(opts: &FormattingOptions) -> FormatConfig {
    let mut cfg = FormatConfig::default();

    let tab = opts.tab_size as usize;
    if tab > 0 {
        cfg.indent_width = tab;
    }

    if let Some(FormattingProperty::Number(n)) = opts.properties.get("maxLineLength") {
        if *n > 0 {
            cfg.max_line_length = *n as usize;
        }
    }

    cfg
}

fn end_position(line_index: &LineIndex, source: &str) -> Position {
    line_index
        .line_col(source.len())
        .map(|lc| Position {
            line: lc.line,
            character: lc.col,
        })
        .unwrap_or(Position {
            line: 0,
            character: 0,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::FormattingOptions;

    fn opts() -> FormattingOptions {
        FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            properties: Default::default(),
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        }
    }

    #[test]
    fn returns_single_edit_for_full_document() {
        let src = "template T(){signal input x;}\n";
        let edits = format_document(src, &opts()).expect("edits");
        assert_eq!(edits.len(), 1);
        let e = &edits[0];
        assert!(e.new_text.contains("template T() {"));
        assert_eq!(e.range.start.line, 0);
        assert_eq!(e.range.start.character, 0);
    }

    #[test]
    fn returns_empty_on_no_change() {
        let src = "pragma circom 2.0.0;\n";
        let edits = format_document(src, &opts()).expect("edits");
        assert_eq!(edits.len(), 0, "already-formatted should produce no edits");
    }

    #[test]
    fn returns_none_on_parse_error() {
        let src = "template {{{ broken";
        let edits = format_document(src, &opts());
        assert!(edits.is_none(), "parse error should suppress formatting");
    }

    #[test]
    fn honours_max_line_length_property() {
        let src = "template T() {\n    signal output z;\n    z <== Foo(aaaa, bbbb, cccc);\n}\n";
        let mut options = opts();
        options
            .properties
            .insert("maxLineLength".to_string(), FormattingProperty::Number(24));
        let edits = format_document(src, &options).expect("edits");
        assert!(!edits.is_empty(), "expected wrapping edit");
        assert!(
            edits[0].new_text.contains("aaaa,\n"),
            "expected wrap:\n{}",
            edits[0].new_text
        );
    }
}
