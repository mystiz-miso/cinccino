use serde_json::Value;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use super::completion;
use super::document_symbol;
use super::signature_help as sig_help;
use super::DocumentStore;
use crate::parser;
use crate::span::LineIndex;
use crate::symbol_table::SymbolTable;

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
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string(), "\"".to_string()]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: Some(vec![",".to_string()]),
                    work_done_progress_options: Default::default(),
                }),
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

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let text = match self.documents.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        let line_index = LineIndex::new(&text);

        // Convert LSP position to byte offset.
        let line = position.line as usize;
        let col = position.character as usize;
        let offset = line_index.offset(line, col).unwrap_or(text.len());

        // Find the call site at the cursor position.
        let call_site = match sig_help::find_call_site(&text, offset) {
            Some(cs) => cs,
            None => return Ok(None),
        };

        // Check for built-in functions first.
        if let Some(help) =
            sig_help::builtin_signature_help(&call_site.name, call_site.active_param)
        {
            return Ok(Some(help));
        }

        // Parse and build a symbol table to look up the definition.
        let (ast, _) = parser::parse(&text);
        let mut symbol_table = SymbolTable::new();
        let file_path = uri.as_str();
        symbol_table.index_file(file_path, &ast);

        Ok(sig_help::signature_help(
            &text,
            offset,
            &symbol_table,
            file_path,
        ))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let text = match self.documents.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        // Convert LSP position to byte offset.
        let offset = match position_to_byte_offset(&text, position) {
            Some(o) => o,
            None => return Ok(None),
        };

        // Parse and build symbol table.
        let file_path = uri.path();
        let (ast, _) = parser::parse(&text);
        let mut table = SymbolTable::new();
        table.index_file(file_path, &ast);

        let ctx = completion::detect_context(&text, offset, &ast, &table, file_path);
        let items = completion::completions(&ctx, &table, file_path);

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn execute_command(&self, _: ExecuteCommandParams) -> Result<Option<Value>> {
        Ok(None)
    }
}

/// Convert an LSP Position (line/character) to a byte offset in source text.
///
/// NOTE: LSP `character` is defined as a UTF-16 code unit offset, but this
/// implementation treats it as a Unicode codepoint count (via `char_indices`).
/// This matches the existing `position_to_offset` in `document.rs` and is
/// correct for Circom sources which are ASCII-only. Supplementary-plane
/// characters (outside BMP) would be handled incorrectly by both functions.
fn position_to_byte_offset(source: &str, position: Position) -> Option<usize> {
    let target_line = position.line as usize;
    let target_col = position.character as usize;
    let mut current_line = 0usize;

    for (i, ch) in source.char_indices() {
        if current_line == target_line {
            // Found the target line start; advance by column.
            let line_start = i;
            for (col, (j, c)) in source[line_start..].char_indices().enumerate() {
                if col == target_col {
                    return Some(line_start + j);
                }
                if c == '\n' {
                    break;
                }
            }
            // Column past end of line or at end of source.
            return Some(
                (line_start
                    + source[line_start..]
                        .find('\n')
                        .unwrap_or(source[line_start..].len()))
                .min(source.len()),
            );
        }
        if ch == '\n' {
            current_line += 1;
        }
    }
    // Line past end of source.
    Some(source.len())
}
