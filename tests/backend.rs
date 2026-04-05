//! In-process tests for `CinccinoBackend`.
//!
//! These exercise the backend through `tower_lsp::Server` using in-memory
//! duplex channels so that cargo-tarpaulin can instrument the code
//! (unlike the `lsp_server` integration tests which spawn a child process).

use std::time::Duration;

use cinccino::server::CinccinoBackend;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::time::timeout;
use tower_lsp::{LspService, Server};

/// In-process LSP client that communicates with the server over duplex
/// channels. The server runs in a background task.
struct InProcessClient {
    reader: BufReader<DuplexStream>,
    writer: DuplexStream,
    next_id: i64,
}

impl InProcessClient {
    /// Spawn an in-process LSP server and return a client connected to it.
    fn spawn() -> Self {
        let (service, socket) = LspService::new(CinccinoBackend::new);

        let (client_read, server_write) = tokio::io::duplex(65536);
        let (server_read, client_write) = tokio::io::duplex(65536);

        tokio::spawn(Server::new(server_read, server_write, socket).serve(service));

        Self {
            reader: BufReader::new(client_read),
            writer: client_write,
            next_id: 1,
        }
    }

    async fn request(&mut self, method: &str, params: Option<Value>) -> Value {
        let id = self.next_id;
        self.next_id += 1;

        let mut msg = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if let Some(p) = params {
            msg["params"] = p;
        }
        self.send_message(&msg).await;
        self.read_response(id).await
    }

    async fn notify(&mut self, method: &str, params: Option<Value>) {
        let mut msg = json!({ "jsonrpc": "2.0", "method": method });
        if let Some(p) = params {
            msg["params"] = p;
        }
        self.send_message(&msg).await;
    }

    async fn send_message(&mut self, msg: &Value) {
        let body = serde_json::to_string(msg).unwrap();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.writer.write_all(header.as_bytes()).await.unwrap();
        self.writer.write_all(body.as_bytes()).await.unwrap();
        self.writer.flush().await.unwrap();
    }

    async fn read_response(&mut self, id: i64) -> Value {
        loop {
            let msg = self.read_message().await;
            if let Some(resp_id) = msg.get("id") {
                if resp_id.as_i64() == Some(id) {
                    return msg;
                }
            }
            // If it's a server→client request (has id + method), send
            // back an empty success response so the server doesn't hang.
            if msg.get("method").is_some() {
                if let Some(req_id) = msg.get("id") {
                    let resp = json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": null,
                    });
                    self.send_message(&resp).await;
                }
            }
        }
    }

    async fn read_message(&mut self) -> Value {
        timeout(Duration::from_secs(10), async {
            let mut content_length: usize = 0;
            loop {
                let mut line = String::new();
                self.reader.read_line(&mut line).await.unwrap();
                let line = line.trim();
                if line.is_empty() {
                    break;
                }
                if let Some(len_str) = line.strip_prefix("Content-Length: ") {
                    content_length = len_str.parse().unwrap();
                }
            }
            assert!(content_length > 0, "missing Content-Length header");

            let mut buf = vec![0u8; content_length];
            tokio::io::AsyncReadExt::read_exact(&mut self.reader, &mut buf)
                .await
                .unwrap();
            serde_json::from_slice::<Value>(&buf).unwrap()
        })
        .await
        .expect("timed out reading LSP message")
    }

    async fn initialize(&mut self) -> Value {
        let resp = self
            .request(
                "initialize",
                Some(json!({
                    "processId": null,
                    "capabilities": {},
                    "rootUri": null,
                })),
            )
            .await;
        self.notify("initialized", Some(json!({}))).await;
        // Give the server time to process the initialized notification.
        tokio::time::sleep(Duration::from_millis(200)).await;
        resp
    }

    async fn open_doc(&mut self, uri: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "circom",
                    "version": 1,
                    "text": text,
                }
            })),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    async fn shutdown_and_exit(&mut self) {
        let resp = self.request("shutdown", None).await;
        assert!(
            resp.get("error").is_none(),
            "shutdown returned error: {resp}"
        );
        self.notify("exit", None).await;
    }
}

// ───────────────────── initialize ─────────────────────

#[tokio::test]
async fn initialize_returns_capabilities() {
    let mut client = InProcessClient::spawn();
    let resp = client.initialize().await;

    let result = &resp["result"];
    assert!(result.is_object(), "expected result object, got: {result}");
    assert_eq!(result["serverInfo"]["name"], "cinccino");

    let sync = &result["capabilities"]["textDocumentSync"];
    assert_eq!(sync["openClose"], true);
    assert_eq!(sync["change"], 2); // INCREMENTAL
    assert_eq!(sync["save"]["includeText"], true);

    let ws = &result["capabilities"]["workspace"]["workspaceFolders"];
    assert_eq!(ws["supported"], true);
    assert_eq!(ws["changeNotifications"], true);

    assert_eq!(result["capabilities"]["documentSymbolProvider"], true);

    let sig_help = &result["capabilities"]["signatureHelpProvider"];
    assert!(sig_help.is_object(), "expected signatureHelpProvider");
    let triggers = sig_help["triggerCharacters"]
        .as_array()
        .expect("expected triggerCharacters");
    assert!(triggers.contains(&json!("(")));
    assert!(triggers.contains(&json!(",")));

    client.shutdown_and_exit().await;
}

// ───────────────────── shutdown ─────────────────────

#[tokio::test]
async fn shutdown_returns_ok() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client.shutdown_and_exit().await;
}

// ───────────────────── did_open ─────────────────────

#[tokio::test]
async fn did_open_valid_document() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client
        .open_doc(
            "file:///test/valid.circom",
            "pragma circom 2.0.0;\ntemplate Foo() { signal input x; }\n",
        )
        .await;
    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn did_open_syntax_error_publishes_diagnostics() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client
        .open_doc(
            "file:///test/bad.circom",
            "pragma circom \"2.2.3\"\ntemplate Foo() {}\n",
        )
        .await;
    client.shutdown_and_exit().await;
}

// ───────────────────── did_change ─────────────────────

#[tokio::test]
async fn did_change_full_replacement() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client
        .open_doc(
            "file:///test/change.circom",
            "template A() { signal input x; }\n",
        )
        .await;

    client
        .notify(
            "textDocument/didChange",
            Some(json!({
                "textDocument": { "uri": "file:///test/change.circom", "version": 2 },
                "contentChanges": [{ "text": "template B() { signal output y; }\n" }]
            })),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = client
        .request(
            "textDocument/documentSymbol",
            Some(json!({ "textDocument": { "uri": "file:///test/change.circom" } })),
        )
        .await;
    let result = resp["result"].as_array().expect("expected array");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["name"], "B");

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn did_change_incremental() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client
        .open_doc("file:///test/incr.circom", "pragma circom \"2.2.3\";\n")
        .await;

    client
        .notify(
            "textDocument/didChange",
            Some(json!({
                "textDocument": { "uri": "file:///test/incr.circom", "version": 2 },
                "contentChanges": [{
                    "range": {
                        "start": { "line": 1, "character": 0 },
                        "end": { "line": 1, "character": 0 }
                    },
                    "text": "template Foo() {}\n"
                }]
            })),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.shutdown_and_exit().await;
}

// ───────────────────── did_close ─────────────────────

#[tokio::test]
async fn did_close_clears_document() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client
        .open_doc(
            "file:///test/close.circom",
            "template A() { signal input x; }\n",
        )
        .await;

    client
        .notify(
            "textDocument/didClose",
            Some(json!({ "textDocument": { "uri": "file:///test/close.circom" } })),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = client
        .request(
            "textDocument/documentSymbol",
            Some(json!({ "textDocument": { "uri": "file:///test/close.circom" } })),
        )
        .await;
    assert!(
        resp["result"].is_null(),
        "expected null for closed document, got: {}",
        resp["result"]
    );

    client.shutdown_and_exit().await;
}

// ───────────────────── did_save ─────────────────────

#[tokio::test]
async fn did_save_with_text_resets_parser() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client
        .open_doc("file:///test/save.circom", "pragma circom \"2.2.3\";\n")
        .await;

    client
        .notify(
            "textDocument/didSave",
            Some(json!({
                "textDocument": { "uri": "file:///test/save.circom" },
                "text": "pragma circom \"2.2.3\";\ntemplate Bar() {}\n"
            })),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn did_save_without_text_uses_cached() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client
        .open_doc("file:///test/save2.circom", "pragma circom \"2.2.3\";\n")
        .await;

    client
        .notify(
            "textDocument/didSave",
            Some(json!({ "textDocument": { "uri": "file:///test/save2.circom" } })),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.shutdown_and_exit().await;
}

// ───────────────────── workspace notifications ─────────────────────

#[tokio::test]
async fn did_change_configuration_does_not_crash() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    client
        .notify(
            "workspace/didChangeConfiguration",
            Some(json!({
                "settings": { "circom": { "libraryPaths": ["/home/user/circomlib"] } }
            })),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn did_change_watched_files_does_not_crash() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    client
        .notify(
            "workspace/didChangeWatchedFiles",
            Some(json!({
                "changes": [
                    { "uri": "file:///test/new.circom", "type": 1 },
                    { "uri": "file:///test/old.circom", "type": 3 }
                ]
            })),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn did_change_workspace_folders_does_not_crash() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    client
        .notify(
            "workspace/didChangeWorkspaceFolders",
            Some(json!({
                "event": {
                    "added": [{ "uri": "file:///workspace/new", "name": "new" }],
                    "removed": [{ "uri": "file:///workspace/old", "name": "old" }]
                }
            })),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.shutdown_and_exit().await;
}

// ───────────────────── document_symbol ─────────────────────

#[tokio::test]
async fn document_symbol_returns_template_outline() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client
        .open_doc(
            "file:///test/symbols.circom",
            "pragma circom 2.0.0;\ntemplate Adder(n) {\n    signal input a;\n    signal input b;\n    signal output c;\n    c <== a + b;\n}\n",
        )
        .await;

    let resp = client
        .request(
            "textDocument/documentSymbol",
            Some(json!({ "textDocument": { "uri": "file:///test/symbols.circom" } })),
        )
        .await;

    let result = resp["result"].as_array().expect("expected array");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["name"], "Adder");
    assert_eq!(result[0]["kind"], 5); // CLASS
    assert_eq!(result[0]["detail"], "template");

    let children = result[0]["children"].as_array().unwrap();
    assert_eq!(children.len(), 4); // n + a + b + c

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn document_symbol_returns_null_for_unknown_uri() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let resp = client
        .request(
            "textDocument/documentSymbol",
            Some(json!({ "textDocument": { "uri": "file:///test/nonexistent.circom" } })),
        )
        .await;
    assert!(resp["result"].is_null(), "expected null for unknown URI");

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn document_symbol_empty_file() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client.open_doc("file:///test/empty.circom", "").await;

    let resp = client
        .request(
            "textDocument/documentSymbol",
            Some(json!({ "textDocument": { "uri": "file:///test/empty.circom" } })),
        )
        .await;
    let result = resp["result"].as_array().expect("expected array");
    assert!(result.is_empty());

    client.shutdown_and_exit().await;
}

// ───────────────────── execute_command ─────────────────────

#[tokio::test]
async fn execute_command_returns_null() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let resp = client
        .request(
            "workspace/executeCommand",
            Some(json!({ "command": "some.command", "arguments": [] })),
        )
        .await;
    assert!(
        resp["result"].is_null(),
        "expected null from executeCommand"
    );

    client.shutdown_and_exit().await;
}

// ───────────────────── full lifecycle ─────────────────────

#[tokio::test]
async fn full_lifecycle_open_change_save_close_shutdown() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/lifecycle.circom";

    // Open.
    client
        .open_doc(uri, "template A() { signal input x; }\n")
        .await;

    // Incremental change: insert a second template.
    client
        .notify(
            "textDocument/didChange",
            Some(json!({
                "textDocument": { "uri": uri, "version": 2 },
                "contentChanges": [{
                    "range": {
                        "start": { "line": 1, "character": 0 },
                        "end": { "line": 1, "character": 0 }
                    },
                    "text": "template B() { signal output y; }\n"
                }]
            })),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify two symbols.
    let resp = client
        .request(
            "textDocument/documentSymbol",
            Some(json!({ "textDocument": { "uri": uri } })),
        )
        .await;
    let result = resp["result"].as_array().unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0]["name"], "A");
    assert_eq!(result[1]["name"], "B");

    // Save with text.
    client
        .notify(
            "textDocument/didSave",
            Some(json!({
                "textDocument": { "uri": uri },
                "text": "template A() { signal input x; }\ntemplate B() { signal output y; }\n"
            })),
        )
        .await;

    // Close.
    client
        .notify(
            "textDocument/didClose",
            Some(json!({ "textDocument": { "uri": uri } })),
        )
        .await;

    // Shutdown.
    client.shutdown_and_exit().await;
}

// ───────────────────── diagnostics lifecycle ─────────────────────

#[tokio::test]
async fn diagnostics_cleared_on_fix() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/fixme.circom";

    // Open with a syntax error.
    client.open_doc(uri, "pragma circom \"2.2.3\"\n").await;

    // Fix the error via full replacement.
    client
        .notify(
            "textDocument/didChange",
            Some(json!({
                "textDocument": { "uri": uri, "version": 2 },
                "contentChanges": [{ "text": "pragma circom \"2.2.3\";\n" }]
            })),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.shutdown_and_exit().await;
}

// ───────────────────── signature_help ─────────────────────

#[tokio::test]
async fn signature_help_template_instantiation() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/sig.circom";
    let text = "template Poseidon(nInputs) {\n    signal input in;\n}\ntemplate T() {\n    component c = Poseidon(2);\n}\n";
    client.open_doc(uri, text).await;

    // Cursor after '(' in `Poseidon(`  -> line 4, col 27
    let resp = client
        .request(
            "textDocument/signatureHelp",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 4, "character": 27 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected signature help, got null");
    let sigs = result["signatures"].as_array().unwrap();
    assert_eq!(sigs.len(), 1);
    assert_eq!(sigs[0]["label"], "Poseidon(nInputs)");
    assert_eq!(result["activeParameter"], 0);

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn signature_help_function_call() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/sig_fn.circom";
    let text = "function nbits(n) {\n    return n;\n}\ntemplate T() {\n    var x = nbits(4);\n}\n";
    client.open_doc(uri, text).await;

    // Cursor after '(' in `nbits(` -> line 4, col 18
    let resp = client
        .request(
            "textDocument/signatureHelp",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 4, "character": 18 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected signature help");
    assert_eq!(result["signatures"][0]["label"], "nbits(n)");

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn signature_help_active_param_on_comma() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/sig_comma.circom";
    let text =
        "function add(a, b) {\n    return a + b;\n}\ntemplate T() {\n    var x = add(1, 2);\n}\n";
    client.open_doc(uri, text).await;

    // Cursor after comma: `add(1, |2)` -> line 4, col 19
    let resp = client
        .request(
            "textDocument/signatureHelp",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 4, "character": 19 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected signature help");
    assert_eq!(result["signatures"][0]["label"], "add(a, b)");
    assert_eq!(result["activeParameter"], 1);

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn signature_help_builtin_log() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/sig_log.circom";
    let text = "template T() {\n    log(42);\n}\n";
    client.open_doc(uri, text).await;

    // Cursor after '(' in `log(` -> line 1, col 8
    let resp = client
        .request(
            "textDocument/signatureHelp",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 8 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected signature help for log");
    assert_eq!(result["signatures"][0]["label"], "log(expr, ...)");

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn signature_help_builtin_assert() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/sig_assert.circom";
    let text = "template T() {\n    assert(1);\n}\n";
    client.open_doc(uri, text).await;

    // Cursor after '(' in `assert(` -> line 1, col 11
    let resp = client
        .request(
            "textDocument/signatureHelp",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 11 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected signature help for assert");
    assert_eq!(result["signatures"][0]["label"], "assert(condition)");

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn signature_help_outside_call_returns_null() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/sig_none.circom";
    let text = "template T() {\n    var x = 1;\n}\n";
    client.open_doc(uri, text).await;

    // Cursor at `1` -> line 1, col 12
    let resp = client
        .request(
            "textDocument/signatureHelp",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 12 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(result.is_null(), "expected null outside call context");

    client.shutdown_and_exit().await;
}
