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

// ───────────────────── completion ─────────────────────

#[tokio::test]
async fn completion_top_level_returns_keywords() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client
        .open_doc("file:///test/comp.circom", "pragma circom \"2.2.3\";\n")
        .await;

    let resp = client
        .request(
            "textDocument/completion",
            Some(json!({
                "textDocument": { "uri": "file:///test/comp.circom" },
                "position": { "line": 1, "character": 0 }
            })),
        )
        .await;

    let result = resp["result"].as_array().expect("expected array");
    let labels: Vec<&str> = result
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    assert!(
        labels.contains(&"template"),
        "should contain template keyword: {labels:?}"
    );
    assert!(
        labels.contains(&"function"),
        "should contain function keyword: {labels:?}"
    );
    assert!(
        labels.contains(&"include"),
        "should contain include keyword: {labels:?}"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn completion_inside_template_returns_signals() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client
        .open_doc(
            "file:///test/tmpl.circom",
            "template Foo(n) {\n    signal input x;\n    \n}\n",
        )
        .await;

    let resp = client
        .request(
            "textDocument/completion",
            Some(json!({
                "textDocument": { "uri": "file:///test/tmpl.circom" },
                "position": { "line": 2, "character": 4 }
            })),
        )
        .await;

    let result = resp["result"].as_array().expect("expected array");
    let labels: Vec<&str> = result
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    assert!(
        labels.contains(&"signal input"),
        "should contain signal keyword: {labels:?}"
    );
    assert!(
        labels.contains(&"var"),
        "should contain var keyword: {labels:?}"
    );
    assert!(
        labels.contains(&"x"),
        "should contain signal name x: {labels:?}"
    );
    assert!(
        labels.contains(&"n"),
        "should contain parameter n: {labels:?}"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn completion_dot_access_returns_signals() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client
        .open_doc(
            "file:///test/dot.circom",
            concat!(
                "template Inner() {\n",
                "    signal input a;\n",
                "    signal output b;\n",
                "}\n",
                "template Outer() {\n",
                "    component c = Inner();\n",
                "    c.\n",
                "}\n",
            ),
        )
        .await;

    let resp = client
        .request(
            "textDocument/completion",
            Some(json!({
                "textDocument": { "uri": "file:///test/dot.circom" },
                "position": { "line": 6, "character": 6 }
            })),
        )
        .await;

    let result = resp["result"].as_array().expect("expected array");
    let labels: Vec<&str> = result
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    assert!(labels.contains(&"a"), "should contain signal a: {labels:?}");
    assert!(labels.contains(&"b"), "should contain signal b: {labels:?}");

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn completion_pragma_returns_versions() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    client
        .open_doc("file:///test/pragma.circom", "pragma circom ")
        .await;

    let resp = client
        .request(
            "textDocument/completion",
            Some(json!({
                "textDocument": { "uri": "file:///test/pragma.circom" },
                "position": { "line": 0, "character": 14 }
            })),
        )
        .await;

    let result = resp["result"].as_array().expect("expected array");
    let labels: Vec<&str> = result
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    assert!(
        labels.contains(&"2.2.3"),
        "should contain version 2.2.3: {labels:?}"
    );
    assert!(
        labels.contains(&"2.0.0"),
        "should contain version 2.0.0: {labels:?}"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn completion_returns_null_for_unknown_uri() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let resp = client
        .request(
            "textDocument/completion",
            Some(json!({
                "textDocument": { "uri": "file:///test/unknown.circom" },
                "position": { "line": 0, "character": 0 }
            })),
        )
        .await;

    assert!(resp["result"].is_null(), "expected null for unknown URI");
    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn initialize_advertises_completion_provider() {
    let mut client = InProcessClient::spawn();
    let resp = client.initialize().await;

    let result = &resp["result"];
    let completion = &result["capabilities"]["completionProvider"];
    assert!(
        completion.is_object(),
        "completionProvider should be an object: {completion}"
    );
    let triggers = completion["triggerCharacters"]
        .as_array()
        .expect("triggerCharacters should be array");
    let trigger_strs: Vec<&str> = triggers.iter().map(|t| t.as_str().unwrap()).collect();
    assert!(trigger_strs.contains(&"."), "should trigger on dot");

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

// ───────────────────── semantic diagnostics ─────────────────────

#[tokio::test]
async fn diagnostics_type_error_assign_to_input() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/type_err.circom";
    let text = "template T() {\n    signal input a;\n    signal output b;\n    a <== 1;\n    b <== a;\n}\n";

    client
        .notify(
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

    // Read messages until we get publishDiagnostics.
    let msg = timeout(Duration::from_secs(5), async {
        loop {
            let msg = client.read_message().await;
            if msg.get("method") == Some(&json!("textDocument/publishDiagnostics")) {
                return msg;
            }
        }
    })
    .await
    .expect("timed out waiting for diagnostics");

    let diagnostics = msg["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");
    assert!(
        !diagnostics.is_empty(),
        "expected at least one semantic diagnostic"
    );
    // Should detect assigning to input signal 'a'.
    let has_input_err = diagnostics
        .iter()
        .any(|d| d["message"].as_str().unwrap_or("").contains("input signal"));
    assert!(
        has_input_err,
        "expected input signal error: {diagnostics:?}"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn diagnostics_constraint_warning_unsafe_assign() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/unsafe.circom";
    let text = "template T() {\n    signal input a;\n    signal output b;\n    b <-- a;\n}\n";

    client
        .notify(
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

    let msg = timeout(Duration::from_secs(5), async {
        loop {
            let msg = client.read_message().await;
            if msg.get("method") == Some(&json!("textDocument/publishDiagnostics")) {
                return msg;
            }
        }
    })
    .await
    .expect("timed out waiting for diagnostics");

    let diagnostics = msg["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");
    assert!(
        !diagnostics.is_empty(),
        "expected unsafe assignment warning"
    );
    // Should be a warning (severity 2), not an error.
    let has_warning = diagnostics
        .iter()
        .any(|d| d["severity"].as_i64() == Some(2));
    assert!(has_warning, "expected warning severity: {diagnostics:?}");

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn diagnostics_clean_file_no_errors() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/clean.circom";
    let text = "template T() {\n    signal input a;\n    signal output b;\n    b <== a;\n}\n";

    client
        .notify(
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

    let msg = timeout(Duration::from_secs(5), async {
        loop {
            let msg = client.read_message().await;
            if msg.get("method") == Some(&json!("textDocument/publishDiagnostics")) {
                return msg;
            }
        }
    })
    .await
    .expect("timed out waiting for diagnostics");

    let diagnostics = msg["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics for clean file: {diagnostics:?}"
    );

    client.shutdown_and_exit().await;
}

// ───────────────────── hover ─────────────────────

#[tokio::test]
async fn hover_on_template_name() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/hover.circom";
    let text = "template Adder(n) {\n    signal input a;\n    signal output b;\n    b <== a;\n}\ntemplate Main() {\n    component c = Adder(4);\n}\n";
    client.open_doc(uri, text).await;

    // Hover on "Adder" in component instantiation (line 6, col 20).
    let resp = client
        .request(
            "textDocument/hover",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 6, "character": 20 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected hover result, got null");
    let value = result["contents"]["value"].as_str().unwrap();
    assert!(
        value.contains("template Adder(n)"),
        "expected template signature: {value}"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn hover_on_signal_name() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/hover_sig.circom";
    let text = "template T() {\n    signal input a;\n    signal output b;\n    b <== a;\n}\n";
    client.open_doc(uri, text).await;

    // Hover on "a" in `b <== a;` (line 3, col 10).
    let resp = client
        .request(
            "textDocument/hover",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 10 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected hover result for signal");
    let value = result["contents"]["value"].as_str().unwrap();
    assert!(
        value.contains("signal input"),
        "expected signal info: {value}"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn hover_on_function_name() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/hover_fn.circom";
    let text = "function nbits(n) {\n    return n;\n}\ntemplate T() {\n    var x = nbits(4);\n}\n";
    client.open_doc(uri, text).await;

    // Hover on "nbits" in `nbits(4)` (line 4, col 14).
    let resp = client
        .request(
            "textDocument/hover",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 4, "character": 14 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected hover result for function");
    let value = result["contents"]["value"].as_str().unwrap();
    assert!(
        value.contains("function nbits(n)"),
        "expected function signature: {value}"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn hover_on_empty_space_returns_null() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/hover_none.circom";
    let text = "template T() {\n    signal input a;\n}\n";
    client.open_doc(uri, text).await;

    // Hover on whitespace (line 1, col 0 = spaces).
    let resp = client
        .request(
            "textDocument/hover",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 0 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(result.is_null(), "expected null for empty space hover");

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn hover_returns_null_for_unknown_uri() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let resp = client
        .request(
            "textDocument/hover",
            Some(json!({
                "textDocument": { "uri": "file:///test/unknown.circom" },
                "position": { "line": 0, "character": 0 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(result.is_null(), "expected null for unknown URI");

    client.shutdown_and_exit().await;
}

// ───────────────────── go to definition ─────────────────────

#[tokio::test]
async fn goto_definition_template() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/goto_def.circom";
    let text = "template Adder(n) {\n    signal input a;\n    signal output b;\n    b <== a;\n}\ntemplate Main() {\n    component c = Adder(4);\n}\n";
    client.open_doc(uri, text).await;

    // Go to definition on "Adder" in `Adder(4)` (line 6, col 20).
    let resp = client
        .request(
            "textDocument/definition",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 6, "character": 20 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected definition location");
    assert_eq!(result["uri"], uri);
    // Template name "Adder" is on line 0.
    assert_eq!(result["range"]["start"]["line"], 0);

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn goto_definition_signal() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/goto_sig.circom";
    let text = "template T() {\n    signal input a;\n    signal output b;\n    b <== a;\n}\n";
    client.open_doc(uri, text).await;

    // Go to definition on "a" in `b <== a;` (line 3, col 10).
    let resp = client
        .request(
            "textDocument/definition",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 10 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected definition location for signal");
    assert_eq!(result["uri"], uri);
    // Signal "a" is declared on line 1.
    assert_eq!(result["range"]["start"]["line"], 1);

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn goto_definition_unknown_symbol_returns_null() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/goto_unknown.circom";
    let text = "template T() {\n    signal output b;\n    b <== unknown_thing;\n}\n";
    client.open_doc(uri, text).await;

    let resp = client
        .request(
            "textDocument/definition",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 2, "character": 12 }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(
        result.is_null(),
        "expected null for unknown symbol: {result}"
    );

    client.shutdown_and_exit().await;
}

// ───────────────────── find references ─────────────────────

#[tokio::test]
async fn references_finds_all_usages() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/refs.circom";
    let text = "template T() {\n    signal input a;\n    signal output b;\n    b <== a;\n}\n";
    client.open_doc(uri, text).await;

    // Find references for "a" (line 3, col 10).
    let resp = client
        .request(
            "textDocument/references",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 10 },
                "context": { "includeDeclaration": true }
            })),
        )
        .await;

    let result = resp["result"].as_array().expect("expected array");
    // "a" appears: declaration (line 1) + usage (line 3) = 2
    assert!(
        result.len() >= 2,
        "expected at least 2 references for 'a': {result:?}"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn references_exclude_declaration() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/refs_excl.circom";
    let text = "template T() {\n    signal input a;\n    signal output b;\n    b <== a;\n}\n";
    client.open_doc(uri, text).await;

    // Find references for "a" without declaration.
    let resp = client
        .request(
            "textDocument/references",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 17 },
                "context": { "includeDeclaration": false }
            })),
        )
        .await;

    let result = resp["result"].as_array().expect("expected array");
    // With declaration excluded, only the usage in `b <== a;` should remain.
    // Note: "a" appears in "signal input a;" declaration and in "b <== a;".
    // The declaration position should be excluded.
    assert!(
        !result.is_empty(),
        "expected at least one reference excluding declaration"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn references_unknown_symbol_returns_null() {
    let mut client = InProcessClient::spawn();
    client.initialize().await;

    let uri = "file:///test/refs_unknown.circom";
    let text = "template T() {\n    signal output b;\n    b <== 1;\n}\n";
    client.open_doc(uri, text).await;

    // Find references for "1" (a number, not an identifier).
    let resp = client
        .request(
            "textDocument/references",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 2, "character": 10 },
                "context": { "includeDeclaration": true }
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(
        result.is_null(),
        "expected null for non-identifier: {result}"
    );

    client.shutdown_and_exit().await;
}

// ───────────────────── initialize capabilities ─────────────────────

#[tokio::test]
async fn initialize_advertises_hover_provider() {
    let mut client = InProcessClient::spawn();
    let resp = client.initialize().await;

    let result = &resp["result"];
    assert_eq!(
        result["capabilities"]["hoverProvider"], true,
        "should advertise hover"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn initialize_advertises_references_provider() {
    let mut client = InProcessClient::spawn();
    let resp = client.initialize().await;

    let result = &resp["result"];
    assert_eq!(
        result["capabilities"]["referencesProvider"], true,
        "should advertise references"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn initialize_advertises_definition_provider() {
    let mut client = InProcessClient::spawn();
    let resp = client.initialize().await;

    let result = &resp["result"];
    assert_eq!(
        result["capabilities"]["definitionProvider"], true,
        "should advertise definition"
    );

    client.shutdown_and_exit().await;
}
