use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::timeout;

/// Helper to spawn the cinccino-lsp binary and communicate via JSON-RPC over
/// stdin/stdout.
struct LspClient {
    child: Child,
    reader: BufReader<tokio::process::ChildStdout>,
    writer: tokio::process::ChildStdin,
    next_id: i64,
}

impl LspClient {
    async fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cinccino-lsp"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("failed to spawn cinccino-lsp");

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        Self {
            child,
            reader: BufReader::new(stdout),
            writer: stdin,
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
        resp
    }

    async fn shutdown_and_exit(mut self) {
        let resp = self.request("shutdown", None).await;
        assert!(
            resp.get("error").is_none(),
            "shutdown returned error: {resp}"
        );
        self.notify("exit", None).await;
        drop(self.writer);

        match timeout(Duration::from_secs(2), self.child.wait()).await {
            Ok(Ok(_)) => {}
            _ => {
                let _ = self.child.kill().await;
            }
        }
    }
}

#[tokio::test]
async fn test_initialize_returns_capabilities() {
    let mut client = LspClient::spawn().await;
    let resp = client.initialize().await;

    let result = &resp["result"];
    assert!(result.is_object(), "expected result object, got: {result}");

    assert_eq!(result["serverInfo"]["name"], "cinccino");

    let caps = &result["capabilities"];
    let sync = &caps["textDocumentSync"];
    assert_eq!(sync["openClose"], true);
    assert_eq!(sync["change"], 2); // INCREMENTAL
    assert_eq!(sync["save"]["includeText"], true);

    assert_eq!(caps["workspace"]["workspaceFolders"]["supported"], true);
    assert_eq!(
        caps["workspace"]["workspaceFolders"]["changeNotifications"],
        true
    );

    assert_eq!(caps["documentSymbolProvider"], true);

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_document_symbol_returns_template_outline() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    let uri = "file:///test/symbols.circom";
    let text = "pragma circom 2.0.0;\ntemplate Adder(n) {\n    signal input a;\n    signal input b;\n    signal output c;\n    c <== a + b;\n}\n";

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "circom",
                    "version": 1,
                    "text": text
                }
            })),
        )
        .await;

    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client
        .request(
            "textDocument/documentSymbol",
            Some(json!({
                "textDocument": { "uri": uri }
            })),
        )
        .await;

    let result = resp["result"].as_array().expect("expected array result");
    assert_eq!(result.len(), 1, "expected 1 top-level symbol (template)");

    let adder = &result[0];
    assert_eq!(adder["name"], "Adder");
    assert_eq!(adder["kind"], 5); // SymbolKind::CLASS = 5
    assert_eq!(adder["detail"], "template");

    let children = adder["children"].as_array().unwrap();
    // n (param) + a, b (input) + c (output) = 4
    assert_eq!(children.len(), 4);
    assert_eq!(children[0]["name"], "n");
    assert_eq!(children[1]["name"], "a");
    assert_eq!(children[1]["kind"], 8); // SymbolKind::FIELD = 8
    assert_eq!(children[2]["name"], "b");
    assert_eq!(children[3]["name"], "c");

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_document_symbol_empty_file() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    let uri = "file:///test/empty.circom";

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "circom",
                    "version": 1,
                    "text": ""
                }
            })),
        )
        .await;

    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client
        .request(
            "textDocument/documentSymbol",
            Some(json!({
                "textDocument": { "uri": uri }
            })),
        )
        .await;

    let result = resp["result"].as_array().expect("expected array result");
    assert!(result.is_empty(), "expected empty symbols for empty file");

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_document_symbol_updates_on_change() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    let uri = "file:///test/update.circom";

    // Open with one template
    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "circom",
                    "version": 1,
                    "text": "template A() { signal input x; }\n"
                }
            })),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client
        .request(
            "textDocument/documentSymbol",
            Some(json!({ "textDocument": { "uri": uri } })),
        )
        .await;
    let result = resp["result"].as_array().unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["name"], "A");

    // Replace content with two templates
    client
        .notify(
            "textDocument/didChange",
            Some(json!({
                "textDocument": { "uri": uri, "version": 2 },
                "contentChanges": [{
                    "text": "template A() { signal input x; }\ntemplate B() { signal output y; }\n"
                }]
            })),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

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

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_shutdown_then_exit() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;
    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_document_open_change_close() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": "file:///test/main.circom",
                    "languageId": "circom",
                    "version": 1,
                    "text": "pragma circom \"2.2.3\";\n"
                }
            })),
        )
        .await;

    tokio::time::sleep(Duration::from_millis(100)).await;

    client
        .notify(
            "textDocument/didChange",
            Some(json!({
                "textDocument": {
                    "uri": "file:///test/main.circom",
                    "version": 2
                },
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

    client
        .notify(
            "textDocument/didClose",
            Some(json!({
                "textDocument": {
                    "uri": "file:///test/main.circom"
                }
            })),
        )
        .await;

    tokio::time::sleep(Duration::from_millis(100)).await;
    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_diagnostics_on_syntax_error() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": "file:///test/bad.circom",
                    "languageId": "circom",
                    "version": 1,
                    "text": "pragma circom \"2.2.3\"\ntemplate Foo() {}\n"
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

    let params = &msg["params"];
    assert_eq!(params["uri"], "file:///test/bad.circom");
    let diagnostics = params["diagnostics"].as_array().unwrap();
    assert!(
        !diagnostics.is_empty(),
        "expected at least one diagnostic for syntax error"
    );
    assert_eq!(diagnostics[0]["severity"], 1); // ERROR
    assert_eq!(diagnostics[0]["source"], "cinccino");

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_configuration_change_does_not_crash() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    client
        .notify(
            "workspace/didChangeConfiguration",
            Some(json!({
                "settings": {
                    "circom": {
                        "libraryPaths": ["/home/user/circomlib"]
                    }
                }
            })),
        )
        .await;

    tokio::time::sleep(Duration::from_millis(100)).await;
    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_did_save_with_text() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": "file:///test/save.circom",
                    "languageId": "circom",
                    "version": 1,
                    "text": "pragma circom \"2.2.3\";\n"
                }
            })),
        )
        .await;

    tokio::time::sleep(Duration::from_millis(100)).await;

    client
        .notify(
            "textDocument/didSave",
            Some(json!({
                "textDocument": {
                    "uri": "file:///test/save.circom"
                },
                "text": "pragma circom \"2.2.3\";\ntemplate Bar() {}\n"
            })),
        )
        .await;

    tokio::time::sleep(Duration::from_millis(100)).await;
    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_signature_help_on_template_call() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    let uri = "file:///test/sig_help.circom";
    let text = "template Poseidon(nInputs) {\n    signal input in;\n}\ntemplate T() {\n    component c = Poseidon(2);\n}\n";

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "circom",
                    "version": 1,
                    "text": text
                }
            })),
        )
        .await;

    tokio::time::sleep(Duration::from_millis(200)).await;

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
    assert!(!result.is_null(), "expected signature help");
    assert_eq!(result["signatures"][0]["label"], "Poseidon(nInputs)");
    assert_eq!(result["activeParameter"], 0);

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_signature_help_returns_null_outside_call() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    let uri = "file:///test/sig_none.circom";
    let text = "template T() {\n    var x = 1;\n}\n";

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "circom",
                    "version": 1,
                    "text": text
                }
            })),
        )
        .await;

    tokio::time::sleep(Duration::from_millis(200)).await;

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
    assert!(result.is_null(), "expected null outside call");

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_initialize_advertises_formatting_capability() {
    let mut client = LspClient::spawn().await;
    let resp = client.initialize().await;
    assert_eq!(
        resp["result"]["capabilities"]["documentFormattingProvider"], true,
        "server should advertise documentFormattingProvider",
    );
    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_formatting_returns_full_document_edit() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    let uri = "file:///test/format.circom";
    let text = "template T(){signal input x;signal output y;y<==x;}\n";

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "circom",
                    "version": 1,
                    "text": text
                }
            })),
        )
        .await;

    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client
        .request(
            "textDocument/formatting",
            Some(json!({
                "textDocument": { "uri": uri },
                "options": { "tabSize": 4, "insertSpaces": true }
            })),
        )
        .await;

    let edits = resp["result"].as_array().expect("expected edits array");
    assert_eq!(edits.len(), 1, "expected a single full-document edit");
    let new_text = edits[0]["newText"].as_str().unwrap();
    assert!(new_text.contains("template T() {"));
    assert!(new_text.contains("    signal input x;"));
    assert!(new_text.contains("    y <== x;"));

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_formatting_preserves_comments_over_lsp() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    let uri = "file:///test/format_comments.circom";
    let text = "\
template T() {\n\
    // doc for x\n\
    signal input x; // inline\n\
}\n\
// end-of-file\n";

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "circom",
                    "version": 1,
                    "text": text
                }
            })),
        )
        .await;

    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client
        .request(
            "textDocument/formatting",
            Some(json!({
                "textDocument": { "uri": uri },
                "options": { "tabSize": 4, "insertSpaces": true }
            })),
        )
        .await;

    let edits = resp["result"].as_array().expect("expected edits array");
    // Already-formatted enough that there might still be an edit; if
    // there is, check the newText preserves comments. If not, the
    // source already contains them.
    let effective = if edits.is_empty() {
        text.to_string()
    } else {
        edits[0]["newText"].as_str().unwrap().to_string()
    };
    assert!(effective.contains("// doc for x"));
    assert!(effective.contains("// inline"));
    assert!(effective.contains("// end-of-file"));

    client.shutdown_and_exit().await;
}
