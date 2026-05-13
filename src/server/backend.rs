use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::RwLock;

use serde_json::Value;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use super::call_hierarchy as ch;
use super::code_action as ca;
use super::completion;
use super::document_symbol;
use super::formatting as fmt_handler;
use super::hover;
use super::rename as rn;
use super::signature_help as sig_help;
use super::DocumentStore;
use crate::ast::File;
use crate::parser;
use crate::span::{LineCol, LineIndex};
use crate::symbol::DiagnosticKind;
use crate::symbol_table::SymbolTable;

/// The cinccino LSP server backend.
pub struct CinccinoBackend {
    client: Client,
    documents: DocumentStore,
    /// Library directories searched after the including file's own
    /// directory when resolving `include "..."`. Populated from the
    /// `libraryPaths` initialization option.
    library_paths: RwLock<Vec<String>>,
}

impl CinccinoBackend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DocumentStore::new(),
            library_paths: RwLock::new(Vec::new()),
        }
    }

    fn lib_dirs(&self) -> Vec<String> {
        self.library_paths.read().unwrap().clone()
    }

    /// Best-effort conversion from an LSP URI string to a filesystem path
    /// suitable for include resolution. URIs like `file:///foo/bar.circom`
    /// become `/foo/bar.circom`; everything else is returned unchanged so
    /// the existing tests (which pass bare paths) keep working.
    fn uri_to_fs_path(uri: &str) -> String {
        if let Ok(url) = Url::parse(uri) {
            if let Ok(p) = url.to_file_path() {
                return p.to_string_lossy().into_owned();
            }
        }
        uri.to_string()
    }

    /// After the root file is indexed, walk its `include` graph
    /// transitively. Each included file's text comes from the matching
    /// open buffer if VS Code has one (so unsaved edits win), otherwise
    /// from disk.
    ///
    /// Indexing key is the resolved `file://` URI of the file on disk —
    /// the same string that goto-definition needs to return as a
    /// `Location`. Each file's `includes` list is rewritten to point at
    /// those URIs (rather than the raw strings from the AST) so the BFS
    /// in `lookup_with_includes` connects correctly.
    fn populate_transitive_includes(
        &self,
        table: &mut SymbolTable,
        root_uri: &str,
        open_docs: &HashMap<String, String>,
    ) {
        let lib_dirs = self.lib_dirs();
        let root_fs = Self::uri_to_fs_path(root_uri);

        // Rewrite the root file's raw include strings to resolved URIs
        // (falling back to the raw string if unresolved so length stays
        // aligned with what's in the AST).
        let rewritten_root = Self::resolve_includes_for(table, root_uri, &root_fs, &lib_dirs);
        table.replace_includes(root_uri, rewritten_root.clone());

        // BFS over resolved-URI keys. visited prevents revisits both in
        // the cyclic-include case and when two parents include the same
        // file.
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(root_uri.to_string());

        let mut queue: VecDeque<String> = VecDeque::new();
        for u in rewritten_root {
            if visited.insert(u.clone()) {
                queue.push_back(u);
            }
        }

        while let Some(target_uri) = queue.pop_front() {
            let target_fs = Self::uri_to_fs_path(&target_uri);

            // Prefer the open buffer over the file on disk so unsaved
            // edits in another tab are reflected in diagnostics.
            let text = open_docs
                .get(&target_uri)
                .cloned()
                .or_else(|| std::fs::read_to_string(&target_fs).ok());
            let Some(text) = text else { continue };

            let (ast, _) = parser::parse(&text);
            table.index_file(&target_uri, &ast);

            let rewritten =
                Self::resolve_includes_for(table, &target_uri, &target_fs, &lib_dirs);
            table.replace_includes(&target_uri, rewritten.clone());

            for u in rewritten {
                if visited.insert(u.clone()) {
                    queue.push_back(u);
                }
            }
        }
    }

    /// Map the freshly-indexed file's raw include strings to resolved
    /// `file://` URIs. Unresolvable includes (e.g. `circomlib/...` with
    /// no matching `libraryPaths` entry) keep their raw string so they
    /// surface as undeclared-symbol diagnostics rather than silently
    /// disappearing.
    fn resolve_includes_for(
        table: &SymbolTable,
        file_key: &str,
        file_fs: &str,
        lib_dirs: &[String],
    ) -> Vec<String> {
        let raw = table.includes(file_key).map(|s| s.to_vec()).unwrap_or_default();
        raw.into_iter()
            .map(|inc| match table.resolve_include_path(&inc, file_fs, lib_dirs) {
                Some(p) => Url::from_file_path(&p).map(|u| u.to_string()).unwrap_or(inc),
                None => inc,
            })
            .collect()
    }

    /// Text for a target file: open buffer first, then disk. Used by
    /// navigation handlers to translate byte spans into line/column for
    /// files that aren't open in the editor.
    fn target_text_for_uri(&self, target_uri: &Url) -> Option<String> {
        if let Some(t) = self.documents.get_text(target_uri) {
            return Some(t);
        }
        let path = target_uri.to_file_path().ok()?;
        std::fs::read_to_string(path).ok()
    }

    fn open_docs_map(&self) -> HashMap<String, String> {
        self.documents
            .all_documents()
            .into_iter()
            .map(|(u, t)| (u.to_string(), t))
            .collect()
    }

    /// Resolve `(text, byte_offset, word)` for the identifier at `position`
    /// in document `uri`, or `None` if any step fails.
    fn resolve_word_at_position(
        &self,
        uri: &Url,
        position: Position,
    ) -> Option<(String, usize, String)> {
        let text = self.documents.get_text(uri)?;
        let offset = position_to_byte_offset(&text, position)?;
        let word = word_at_offset(&text, offset);
        if word.is_empty() {
            return None;
        }
        Some((text, offset, word))
    }

    /// Index the current document plus its transitive include closure
    /// (read from disk; open buffers shadow disk content). Returns the
    /// parsed AST of the current file.
    fn build_cross_document_table(&self, uri: &Url, text: &str) -> (File, SymbolTable) {
        let (ast, _) = parser::parse(text);
        let file_path = uri.as_str();
        let mut symbol_table = SymbolTable::new();
        symbol_table.index_file(file_path, &ast);

        let open_docs = self.open_docs_map();
        self.populate_transitive_includes(&mut symbol_table, file_path, &open_docs);
        (ast, symbol_table)
    }

    /// Run semantic checks against the current file plus its transitive
    /// include closure, and collect LSP diagnostics for the current file
    /// only.
    fn collect_semantic_diagnostics(
        &self,
        uri: &Url,
        text: &str,
        line_index: &LineIndex,
    ) -> Vec<Diagnostic> {
        let (ast, _) = parser::parse(text);
        let file_path = uri.as_str();
        let mut table = SymbolTable::new();
        table.index_file(file_path, &ast);

        let open_docs = self.open_docs_map();
        self.populate_transitive_includes(&mut table, file_path, &open_docs);

        // Run type checker and constraint checker.
        table.check_types(file_path, &ast);
        table.check_constraints(file_path, &ast);
        table.check_underconstrained(file_path, &ast);
        table.check_undeclared(file_path, &ast);

        // Convert semantic diagnostics to LSP diagnostics.
        let mut out = Vec::new();
        for diag in table.diagnostics() {
            if diag.file != file_path {
                continue;
            }
            let Some(start) = line_index.line_col(diag.span.start) else {
                continue;
            };
            let Some(end) = line_index.line_col(diag.span.end) else {
                continue;
            };
            out.push(Diagnostic {
                range: lc_range(start, end),
                severity: Some(severity_for(diag.kind)),
                source: Some("cinccino".to_string()),
                message: diag.message.clone(),
                ..Default::default()
            });
        }
        out
    }

    /// Publish diagnostics from cached incremental parse errors plus
    /// semantic checks (type checker + constraint checker).
    async fn publish_diagnostics_cached(&self, uri: Url) {
        if let Some((text, parse_errors)) = self.documents.get_parse_errors(&uri) {
            let line_index = LineIndex::new(&text);

            // Parse-error diagnostics.
            let mut diagnostics: Vec<Diagnostic> = parse_errors
                .into_iter()
                .filter_map(|err| {
                    let start = line_index.line_col(err.span.start)?;
                    let end = line_index.line_col(err.span.end)?;
                    Some(Diagnostic {
                        range: lc_range(start, end),
                        severity: Some(DiagnosticSeverity::ERROR),
                        source: Some("cinccino".to_string()),
                        message: err.message,
                        ..Default::default()
                    })
                })
                .collect();

            // Run semantic checks only when there are no parse errors,
            // so the AST is well-formed.
            if diagnostics.is_empty() {
                diagnostics.extend(self.collect_semantic_diagnostics(&uri, &text, &line_index));
            }

            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }
}

fn lc_range(start: LineCol, end: LineCol) -> Range {
    Range {
        start: Position {
            line: start.line,
            character: start.col,
        },
        end: Position {
            line: end.line,
            character: end.col,
        },
    }
}

fn severity_for(kind: DiagnosticKind) -> DiagnosticSeverity {
    match kind {
        DiagnosticKind::UnsafeSignalAssignment
        | DiagnosticKind::TagLoss
        | DiagnosticKind::UnusedComponentOutput
        | DiagnosticKind::MissingComponentInput
        | DiagnosticKind::UnderconstrainedOutput => DiagnosticSeverity::WARNING,
        _ => DiagnosticSeverity::ERROR,
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for CinccinoBackend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Pull libraryPaths out of {"libraryPaths": ["..."]} initialization
        // options. The extension client sends these from the
        // `cinccino.libraryPaths` user setting.
        if let Some(opts) = &params.initialization_options {
            if let Some(arr) = opts.get("libraryPaths").and_then(|v| v.as_array()) {
                let dirs: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect();
                *self.library_paths.write().unwrap() = dirs;
            }
        }
        Ok(InitializeResult {
            capabilities: server_capabilities(),
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

    #[tracing::instrument(level = "debug", skip_all, fields(uri = %params.text_document.uri))]
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

    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(uri = %params.text_document_position_params.text_document.uri),
    )]
    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let text = match self.documents.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        // Convert LSP position to byte offset
        let offset = match position_to_byte_offset(&text, position) {
            Some(o) => o,
            None => return Ok(None),
        };

        let word = word_at_offset(&text, offset);
        if word.is_empty() {
            return Ok(None);
        }

        let (ast, symbol_table) = self.build_cross_document_table(&uri, &text);
        let file_path = uri.as_str();

        // Find the scope at the cursor position for correct resolution.
        let scope = completion::find_scope_at_offset_ast(&ast, offset, &symbol_table, file_path);

        if let Some(symbol) = symbol_table.lookup_with_includes(scope, &word, file_path) {
            let target_uri = Url::parse(&symbol.file).unwrap_or_else(|_| uri.clone());

            // For cross-file symbols we need the target file's text so
            // we can convert byte spans to line/col. Open buffer first,
            // then disk; only fall through to the current file's text
            // when symbol came from the current file or no disk source
            // exists (shouldn't happen post-fix, but keep safe).
            let target_text = if symbol.file == file_path {
                text.clone()
            } else {
                self.target_text_for_uri(&target_uri)
                    .unwrap_or_else(|| text.clone())
            };
            let target_line_index = LineIndex::new(&target_text);

            let start = target_line_index
                .line_col(symbol.span.start)
                .unwrap_or(LineCol { line: 0, col: 0 });
            let end = target_line_index.line_col(symbol.span.end).unwrap_or(start);

            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                uri: target_uri,
                range: lc_range(start, end),
            })));
        }

        Ok(None)
    }

    /// Circom has no interface/trait/abstract construct — every template
    /// and function has exactly one definition site. "Go to Implementation"
    /// (Ctrl+F12 in VS Code) is therefore identical to "Go to Definition";
    /// we delegate so the two stay in sync. (`GotoImplementationParams` /
    /// `Response` are type aliases for the `GotoDefinition*` types in
    /// `lsp_types::request`, so the signatures are interchangeable.)
    async fn goto_implementation(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        self.goto_definition(params).await
    }

    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(uri = %params.text_document_position_params.text_document.uri),
    )]
    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let text = match self.documents.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        let offset = match position_to_byte_offset(&text, position) {
            Some(o) => o,
            None => return Ok(None),
        };

        let word = word_at_offset(&text, offset);
        if word.is_empty() {
            return Ok(None);
        }

        let (ast, _) = parser::parse(&text);
        let mut symbol_table = SymbolTable::new();
        let file_path = uri.as_str();
        symbol_table.index_file(file_path, &ast);

        // Index other open documents for cross-file resolution.
        for (doc_uri, doc_text) in self.documents.all_documents() {
            if doc_uri != uri {
                let (doc_ast, _) = parser::parse(&doc_text);
                symbol_table.index_file(doc_uri.as_str(), &doc_ast);
            }
        }

        let scope = completion::find_scope_at_offset_ast(&ast, offset, &symbol_table, file_path);

        Ok(hover::hover_info(&symbol_table, scope, &word, file_path))
    }

    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(uri = %params.text_document_position.text_document.uri),
    )]
    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let Some((text, offset, word)) = self.resolve_word_at_position(&uri, position) else {
            return Ok(None);
        };

        let all_docs = self.documents.all_documents();
        let (ast, symbol_table) = self.build_cross_document_table(&uri, &text);
        let file_path = uri.as_str();

        let scope = completion::find_scope_at_offset_ast(&ast, offset, &symbol_table, file_path);

        let Some(target_symbol) = symbol_table.lookup_with_includes(scope, &word, file_path) else {
            return Ok(None);
        };
        let target_name = target_symbol.name.clone();
        let def_file = target_symbol.file.clone();
        let def_start = target_symbol.span.start;
        let include_declaration = params.context.include_declaration;

        let mut locations = Vec::new();
        scan_identifier_occurrences(
            &uri,
            &text,
            &target_name,
            &def_file,
            def_start,
            include_declaration,
            &mut locations,
        );
        for (doc_uri, doc_text) in &all_docs {
            if *doc_uri != uri {
                scan_identifier_occurrences(
                    doc_uri,
                    doc_text,
                    &target_name,
                    &def_file,
                    def_start,
                    include_declaration,
                    &mut locations,
                );
            }
        }

        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locations))
        }
    }

    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(uri = %params.text_document_position_params.text_document.uri),
    )]
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

    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(uri = %params.text_document_position.text_document.uri),
    )]
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

    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(uri = %params.text_document_position.text_document.uri),
    )]
    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        // Validate new name up front.
        if !rn::is_valid_identifier(&new_name) {
            return Err(rn::invalid_rename_error(&format!(
                "'{new_name}' is not a valid Circom identifier"
            )));
        }

        let Some((text, offset, word)) = self.resolve_word_at_position(&uri, position) else {
            return Ok(None);
        };

        let (ast, table) = self.build_cross_document_table(&uri, &text);
        let all_docs = self.documents.all_documents();
        let file_path = uri.as_str();

        // Resolve the symbol under the cursor.
        let scope = completion::find_scope_at_offset_ast(&ast, offset, &table, file_path);
        let target = match table.lookup_with_includes(scope, &word, file_path) {
            Some(s) => s.clone(),
            None => return Ok(None),
        };

        if !rn::is_renameable(&target.kind) {
            return Err(rn::invalid_rename_error("symbol cannot be renamed"));
        }

        // Conflict check: a symbol with `new_name` already lives in the
        // target's defining scope.
        if rn::would_conflict(
            &table,
            target.scope,
            &new_name,
            &target.file,
            target.span.start,
        ) {
            return Err(rn::invalid_rename_error(&format!(
                "cannot rename: '{new_name}' already exists in this scope"
            )));
        }

        let edit = rn::build_workspace_edit(
            &target.name,
            &new_name,
            &target.file,
            target.span.start,
            &all_docs,
        );
        Ok(Some(edit))
    }

    #[tracing::instrument(level = "debug", skip_all, fields(uri = %params.text_document.uri))]
    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let text = match self.documents.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        let actions = ca::code_actions(&uri, &text, &params.context.diagnostics);
        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    #[tracing::instrument(level = "debug", skip_all, fields(uri = %params.text_document.uri))]
    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let text = match self.documents.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };
        Ok(fmt_handler::format_document(&text, &params.options))
    }

    /// Resolve the caller at a cursor position (#383). Returns a
    /// single-item list whose CallHierarchyItem carries the caller's
    /// definition range — the client then threads that item back
    /// through `callHierarchy/outgoingCalls` below.
    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(uri = %params.text_document_position_params.text_document.uri),
    )]
    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let text = match self.documents.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };
        let Some(offset) = position_to_byte_offset(&text, position) else {
            return Ok(None);
        };
        let (file, _errors) = parser::parse(&text);
        let Some(caller) = ch::caller_at_offset(&file, offset) else {
            return Ok(None);
        };
        let line_index = LineIndex::new(&text);
        Ok(Some(vec![ch::caller_to_item(&caller, uri, &line_index)]))
    }

    /// Return every outgoing call from the caller named in `item`
    /// (#383). Callee ranges point at the call site in the caller's
    /// body; callee URIs point at the caller's file — cross-file
    /// resolution is the indexer's bare-name resolver's job.
    #[tracing::instrument(level = "debug", skip_all, fields(uri = %params.item.uri, name = %params.item.name))]
    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        let uri = params.item.uri.clone();
        let text = match self.documents.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };
        let (file, _errors) = parser::parse(&text);
        let kind = match params.item.kind {
            SymbolKind::CLASS => ch::CallerKind::Template,
            _ => ch::CallerKind::Function,
        };
        let outgoing = ch::outgoing_calls_for(&file, &params.item.name, kind);
        if outgoing.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let line_index = LineIndex::new(&text);
        let calls = outgoing
            .iter()
            .map(|o| ch::outgoing_to_call(o, uri.clone(), &line_index, None))
            .collect();
        Ok(Some(calls))
    }
}

/// Scan `doc_text` for every word-boundary occurrence of `name` and push
/// an LSP location for each into `locations`. When `include_declaration`
/// is false, an occurrence at `(def_file, def_start)` is skipped.
#[allow(clippy::too_many_arguments)]
fn scan_identifier_occurrences(
    doc_uri: &Url,
    doc_text: &str,
    name: &str,
    def_file: &str,
    def_start: usize,
    include_declaration: bool,
    locations: &mut Vec<Location>,
) {
    // Walk the lexer's token stream instead of doing a raw text find.
    // The lexer already skips comments (`//` and `/* */`) and exposes
    // string literals as a distinct token, so identifier-shaped text
    // inside comments or strings is never reported as a reference —
    // which was the bug here: doc comments like `/** GTBitVector: … */`
    // were counted as usages of the template.
    let line_index = LineIndex::new(doc_text);
    let (tokens, _errors) = crate::lexer::tokenize(doc_text);
    for spanned in tokens {
        let crate::lexer::Token::Ident(ref ident) = spanned.token else {
            continue;
        };
        if ident != name {
            continue;
        }
        let is_definition = doc_uri.as_str() == def_file && spanned.span.start == def_start;
        if !include_declaration && is_definition {
            continue;
        }
        let (Some(start), Some(end)) = (
            line_index.line_col(spanned.span.start),
            line_index.line_col(spanned.span.end),
        ) else {
            continue;
        };
        locations.push(Location {
            uri: doc_uri.clone(),
            range: lc_range(start, end),
        });
    }
}

/// Build the static `ServerCapabilities` advertised by this LSP.
fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
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
        definition_provider: Some(OneOf::Left(true)),
        implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        references_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            retrigger_characters: Some(vec![",".to_string()]),
            work_done_progress_options: Default::default(),
        }),
        rename_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
        workspace: Some(WorkspaceServerCapabilities {
            workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                supported: Some(true),
                change_notifications: Some(OneOf::Left(true)),
            }),
            file_operations: None,
        }),
        ..Default::default()
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

/// Extract the word (identifier) at a given byte offset in source text.
///
/// A "word" is a contiguous run of alphanumeric characters or underscores.
/// Returns an empty string if the offset is not within a word.
fn word_at_offset(source: &str, offset: usize) -> String {
    let bytes = source.as_bytes();
    if offset >= bytes.len() {
        return String::new();
    }

    fn is_ident(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
    }

    if !is_ident(bytes[offset]) {
        return String::new();
    }

    let mut start = offset;
    while start > 0 && is_ident(bytes[start - 1]) {
        start -= 1;
    }

    let mut end = offset;
    while end < bytes.len() && is_ident(bytes[end]) {
        end += 1;
    }

    source[start..end].to_string()
}
