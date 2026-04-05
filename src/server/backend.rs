use serde_json::Value;
use similar::TextDiff;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use super::document_symbol;
use super::DocumentStore;
use crate::parser;
use crate::pretty_print::{self, FormatConfig};
use crate::span::LineIndex;

use crate::parser::ParseError;

/// The cinccino LSP server backend.
pub struct CinccinoBackend {
    client: Client,
    documents: DocumentStore,
}

impl CinccinoBackend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DocumentStore::new(),
        }
    }

    /// Publish diagnostics from cached incremental parse errors.
    async fn publish_diagnostics_cached(&self, uri: Url) {
        if let Some((text, errors)) = self.documents.get_parse_errors(&uri) {
            self.publish_errors(uri, &text, errors).await;
        }
    }

    /// Convert parse errors to LSP diagnostics and publish them.
    async fn publish_errors(&self, uri: Url, text: &str, errors: Vec<ParseError>) {
        let line_index = LineIndex::new(text);

        let diagnostics: Vec<Diagnostic> = errors
            .into_iter()
            .filter_map(|err| {
                let start = line_index.line_col(err.span.start)?;
                let end = line_index.line_col(err.span.end)?;
                Some(Diagnostic {
                    range: Range {
                        start: Position {
                            line: start.line,
                            character: start.col,
                        },
                        end: Position {
                            line: end.line,
                            character: end.col,
                        },
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("cinccino".to_string()),
                    message: err.message,
                    ..Default::default()
                })
            })
            .collect();

        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for CinccinoBackend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(true),
                        })),
                        ..Default::default()
                    },
                )),
                document_symbol_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "cinccino".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        // Register file watcher for .circom files.
        let registration = Registration {
            id: "circom-file-watcher".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: Some(
                serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                    watchers: vec![FileSystemWatcher {
                        glob_pattern: GlobPattern::String("**/*.circom".to_string()),
                        kind: Some(WatchKind::all()),
                    }],
                })
                .unwrap(),
            ),
        };

        if let Err(err) = self.client.register_capability(vec![registration]).await {
            self.client
                .log_message(
                    MessageType::WARNING,
                    format!("Failed to register file watcher: {err}"),
                )
                .await;
        }

        self.client
            .log_message(MessageType::INFO, "cinccino LSP server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        let version = params.text_document.version;

        self.documents.open(uri.clone(), version, text);
        self.publish_diagnostics_cached(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        self.documents
            .apply_changes(&uri, version, params.content_changes);

        // Use the cached incremental parse result instead of a full
        // re-parse.
        self.publish_diagnostics_cached(uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.close(&uri);

        // Clear diagnostics for closed document.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(text) = params.text {
            // Reset the incremental parser so the next did_change starts
            // from clean state, then re-publish diagnostics.
            self.documents.reset_parser(&uri, &text);
            self.publish_diagnostics_cached(uri).await;
        } else if let Some(text) = self.documents.get_text(&uri) {
            self.documents.reset_parser(&uri, &text);
            self.publish_diagnostics_cached(uri).await;
        }
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        self.client
            .log_message(
                MessageType::INFO,
                format!("Configuration changed: {}", params.settings),
            )
            .await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        for change in &params.changes {
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("File changed: {} ({:?})", change.uri, change.typ),
                )
                .await;
        }
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        for added in &params.event.added {
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("Workspace folder added: {}", added.uri),
                )
                .await;
        }
        for removed in &params.event.removed {
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("Workspace folder removed: {}", removed.uri),
                )
                .await;
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let text = match self.documents.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        let (ast, _errors) = parser::parse(&text);
        let symbols = document_symbol::document_symbols(&ast, &text);

        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let text = match self.documents.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        let config = format_config_from_options(&params.options);
        match compute_formatting_edits(&text, &config, 0, u32::MAX) {
            FormattingResult::Edits(edits) => Ok(Some(edits)),
            FormattingResult::ParseErrors => {
                self.client
                    .show_message(MessageType::WARNING, "Cannot format: file has parse errors")
                    .await;
                Ok(None)
            }
        }
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let text = match self.documents.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        let config = format_config_from_options(&params.options);
        match compute_formatting_edits(
            &text,
            &config,
            params.range.start.line,
            params.range.end.line,
        ) {
            FormattingResult::Edits(edits) => Ok(Some(edits)),
            FormattingResult::ParseErrors => {
                self.client
                    .show_message(MessageType::WARNING, "Cannot format: file has parse errors")
                    .await;
                Ok(None)
            }
        }
    }

    async fn execute_command(&self, _: ExecuteCommandParams) -> Result<Option<Value>> {
        Ok(None)
    }
}

/// Result of computing formatting edits.
enum FormattingResult {
    /// Successfully computed edits (may be empty if text is unchanged).
    Edits(Vec<TextEdit>),
    /// Source has parse errors; formatting is not possible.
    ParseErrors,
}

/// Parse, format, and diff the source text, returning line-level edits
/// restricted to the given line range.
fn compute_formatting_edits(
    text: &str,
    config: &FormatConfig,
    start_line: u32,
    end_line: u32,
) -> FormattingResult {
    let (ast, errors) = parser::parse(text);
    if !errors.is_empty() {
        return FormattingResult::ParseErrors;
    }

    let formatted = pretty_print::format_with_trivia(text, &ast, config);
    if formatted == text {
        return FormattingResult::Edits(Vec::new());
    }

    FormattingResult::Edits(diff_to_range_edits(text, &formatted, start_line, end_line))
}

/// Build a [`FormatConfig`] from LSP [`FormattingOptions`].
///
/// Reads `circom.maxLineLength` from the `properties` map, if present.
fn format_config_from_options(opts: &FormattingOptions) -> FormatConfig {
    let mut config = FormatConfig::from_lsp(opts.tab_size, opts.insert_spaces);
    if let Some(prop) = opts.properties.get("circom.maxLineLength") {
        let max = match prop {
            FormattingProperty::Number(n) if *n > 0 => Some(*n as usize),
            FormattingProperty::String(s) => s.parse::<usize>().ok(),
            _ => None,
        };
        if let Some(m) = max {
            config.max_line_length = Some(m);
        }
    }
    config
}

/// Compute LSP [`TextEdit`]s from a diff between `old` and `new` text,
/// restricted to changes that overlap the line range
/// `[req_start_line, req_end_line]` (inclusive, 0-based).
fn diff_to_range_edits(
    old: &str,
    new: &str,
    req_start_line: u32,
    req_end_line: u32,
) -> Vec<TextEdit> {
    let diff = TextDiff::from_lines(old, new);
    let mut edits = Vec::new();

    // Unified overlap check: an op overlaps the requested range when
    // the first affected old line is at or before req_end_line AND the
    // last affected old line is at or after req_start_line.
    let overlaps =
        |first: u32, last_exclusive: u32| first <= req_end_line && last_exclusive > req_start_line;

    for group in diff.grouped_ops(0) {
        for op in &group {
            match op {
                similar::DiffOp::Equal { .. } => {}
                similar::DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => {
                    let old_start = *old_index as u32;
                    let old_end = (*old_index + *old_len) as u32;
                    if !overlaps(old_start, old_end) {
                        continue;
                    }
                    let new_lines: Vec<&str> =
                        new.lines().skip(*new_index).take(*new_len).collect();
                    let mut new_text = new_lines.join("\n");
                    if !new_text.ends_with('\n') {
                        new_text.push('\n');
                    }
                    edits.push(TextEdit {
                        range: Range {
                            start: Position {
                                line: old_start,
                                character: 0,
                            },
                            end: Position {
                                line: old_end,
                                character: 0,
                            },
                        },
                        new_text,
                    });
                }
                similar::DiffOp::Delete {
                    old_index, old_len, ..
                } => {
                    let old_start = *old_index as u32;
                    let old_end = (*old_index + *old_len) as u32;
                    if !overlaps(old_start, old_end) {
                        continue;
                    }
                    edits.push(TextEdit {
                        range: Range {
                            start: Position {
                                line: old_start,
                                character: 0,
                            },
                            end: Position {
                                line: old_end,
                                character: 0,
                            },
                        },
                        new_text: String::new(),
                    });
                }
                similar::DiffOp::Insert {
                    old_index,
                    new_index,
                    new_len,
                } => {
                    let insert_at = *old_index as u32;
                    // Treat an insert as a zero-width range at insert_at.
                    // Use insert_at + 1 as the exclusive end so the
                    // unified `overlaps` check works consistently.
                    if !overlaps(insert_at, insert_at + 1) {
                        continue;
                    }
                    let new_lines: Vec<&str> =
                        new.lines().skip(*new_index).take(*new_len).collect();
                    let mut new_text = new_lines.join("\n");
                    if !new_text.ends_with('\n') {
                        new_text.push('\n');
                    }
                    edits.push(TextEdit {
                        range: Range {
                            start: Position {
                                line: insert_at,
                                character: 0,
                            },
                            end: Position {
                                line: insert_at,
                                character: 0,
                            },
                        },
                        new_text,
                    });
                }
            }
        }
    }

    edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_opts(
        tab_size: u32,
        insert_spaces: bool,
        properties: HashMap<String, FormattingProperty>,
    ) -> FormattingOptions {
        FormattingOptions {
            tab_size,
            insert_spaces,
            properties,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        }
    }

    #[test]
    fn format_config_defaults_to_4_spaces() {
        let opts = make_opts(4, true, HashMap::new());
        let config = format_config_from_options(&opts);
        assert_eq!(config.indent, "    ");
        assert!(config.max_line_length.is_none());
    }

    #[test]
    fn format_config_tabs() {
        let opts = make_opts(4, false, HashMap::new());
        let config = format_config_from_options(&opts);
        assert_eq!(config.indent, "\t");
    }

    #[test]
    fn format_config_custom_tab_size() {
        let opts = make_opts(2, true, HashMap::new());
        let config = format_config_from_options(&opts);
        assert_eq!(config.indent, "  ");
    }

    #[test]
    fn format_config_reads_max_line_length_number() {
        let mut props = HashMap::new();
        props.insert(
            "circom.maxLineLength".to_string(),
            FormattingProperty::Number(80),
        );
        let opts = make_opts(4, true, props);
        let config = format_config_from_options(&opts);
        assert_eq!(config.max_line_length, Some(80));
    }

    #[test]
    fn format_config_reads_max_line_length_string() {
        let mut props = HashMap::new();
        props.insert(
            "circom.maxLineLength".to_string(),
            FormattingProperty::String("120".to_string()),
        );
        let opts = make_opts(4, true, props);
        let config = format_config_from_options(&opts);
        assert_eq!(config.max_line_length, Some(120));
    }

    #[test]
    fn format_config_ignores_zero_max_line_length() {
        let mut props = HashMap::new();
        props.insert(
            "circom.maxLineLength".to_string(),
            FormattingProperty::Number(0),
        );
        let opts = make_opts(4, true, props);
        let config = format_config_from_options(&opts);
        assert!(config.max_line_length.is_none());
    }

    #[test]
    fn format_config_ignores_invalid_string() {
        let mut props = HashMap::new();
        props.insert(
            "circom.maxLineLength".to_string(),
            FormattingProperty::String("abc".to_string()),
        );
        let opts = make_opts(4, true, props);
        let config = format_config_from_options(&opts);
        assert!(config.max_line_length.is_none());
    }

    #[test]
    fn format_config_ignores_bool_property() {
        let mut props = HashMap::new();
        props.insert(
            "circom.maxLineLength".to_string(),
            FormattingProperty::Bool(true),
        );
        let opts = make_opts(4, true, props);
        let config = format_config_from_options(&opts);
        assert!(config.max_line_length.is_none());
    }

    #[test]
    fn diff_range_edits_identical_text() {
        let text = "line1\nline2\nline3\n";
        let edits = diff_to_range_edits(text, text, 0, 2);
        assert!(edits.is_empty());
    }

    #[test]
    fn diff_range_edits_replace_within_range() {
        let old = "aaa\nbbb\nccc\n";
        let new = "aaa\nBBB\nccc\n";
        let edits = diff_to_range_edits(old, new, 1, 1);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start.line, 1);
        assert_eq!(edits[0].range.end.line, 2);
        assert_eq!(edits[0].new_text, "BBB\n");
    }

    #[test]
    fn diff_range_edits_skips_changes_outside_range() {
        let old = "aaa\nbbb\nccc\n";
        let new = "AAA\nbbb\nCCC\n";
        // Only request range covering line 1 (middle line).
        let edits = diff_to_range_edits(old, new, 1, 1);
        assert!(edits.is_empty(), "changes outside range should be excluded");
    }

    #[test]
    fn diff_range_edits_insert_within_range() {
        let old = "aaa\nccc\n";
        let new = "aaa\nbbb\nccc\n";
        let edits = diff_to_range_edits(old, new, 0, 1);
        assert!(!edits.is_empty(), "insert within range should be included");
    }

    #[test]
    fn diff_range_edits_delete_within_range() {
        let old = "aaa\nbbb\nccc\n";
        let new = "aaa\nccc\n";
        let edits = diff_to_range_edits(old, new, 1, 1);
        assert!(!edits.is_empty(), "delete within range should be included");
        assert!(edits[0].new_text.is_empty());
    }

    #[test]
    fn diff_range_edits_no_spurious_newlines() {
        let old = "aaa\nbbb\nccc\nddd\n";
        let new = "aaa\nBBB\nCCC\nddd\n";
        let edits = diff_to_range_edits(old, new, 0, 3);
        for edit in &edits {
            assert!(
                !edit.new_text.contains("\n\n"),
                "edit should not contain double newlines: {:?}",
                edit.new_text
            );
        }
    }

    #[test]
    fn diff_range_edits_multiple_changes_in_range() {
        let old = "aaa\nbbb\nccc\nddd\neee\n";
        let new = "aaa\nBBB\nccc\nDDD\neee\n";
        let edits = diff_to_range_edits(old, new, 0, 4);
        assert_eq!(
            edits.len(),
            2,
            "two separate changes should produce two edits"
        );
    }

    #[test]
    fn diff_range_edits_insert_at_start() {
        let old = "aaa\nbbb\n";
        let new = "NEW\naaa\nbbb\n";
        let edits = diff_to_range_edits(old, new, 0, 1);
        assert!(!edits.is_empty(), "insert at start should be included");
    }

    #[test]
    fn diff_range_edits_delete_at_end() {
        let old = "aaa\nbbb\nccc\n";
        let new = "aaa\nbbb\n";
        let edits = diff_to_range_edits(old, new, 2, 2);
        assert!(
            !edits.is_empty(),
            "delete at end within range should be included"
        );
        assert!(edits[0].new_text.is_empty());
    }

    #[test]
    fn diff_range_edits_full_replace() {
        let old = "aaa\nbbb\n";
        let new = "ccc\nddd\neee\n";
        let edits = diff_to_range_edits(old, new, 0, 1);
        assert!(!edits.is_empty(), "full replace should produce edits");
    }

    #[test]
    fn format_config_negative_number_ignored() {
        let mut props = HashMap::new();
        props.insert(
            "circom.maxLineLength".to_string(),
            FormattingProperty::Number(-1),
        );
        let opts = make_opts(4, true, props);
        let config = format_config_from_options(&opts);
        assert!(config.max_line_length.is_none());
    }

    #[test]
    fn compute_edits_returns_parse_errors_on_invalid_source() {
        let config = FormatConfig::default();
        let result = compute_formatting_edits("template Foo {", &config, 0, u32::MAX);
        assert!(matches!(result, FormattingResult::ParseErrors));
    }

    #[test]
    fn compute_edits_returns_empty_when_unchanged() {
        let src = "pragma circom 2.0.0;\n";
        let config = FormatConfig::default();
        let result = compute_formatting_edits(src, &config, 0, u32::MAX);
        match result {
            FormattingResult::Edits(edits) => assert!(edits.is_empty()),
            _ => panic!("expected Edits"),
        }
    }

    #[test]
    fn compute_edits_returns_edits_for_formatting_changes() {
        // Extra spaces should be reformatted.
        let src = "pragma    circom    2.0.0;\n";
        let config = FormatConfig::default();
        let result = compute_formatting_edits(src, &config, 0, u32::MAX);
        match result {
            FormattingResult::Edits(edits) => assert!(!edits.is_empty()),
            _ => panic!("expected Edits"),
        }
    }

    #[test]
    fn compute_edits_respects_range() {
        let src = "pragma    circom    2.0.0;\ninclude   \"foo.circom\";\n";
        let config = FormatConfig::default();
        // Only request formatting for line 0.
        let result = compute_formatting_edits(src, &config, 0, 0);
        match result {
            FormattingResult::Edits(edits) => {
                for edit in &edits {
                    assert!(
                        edit.range.start.line <= 0,
                        "edit should be within requested range"
                    );
                }
            }
            _ => panic!("expected Edits"),
        }
    }
}
