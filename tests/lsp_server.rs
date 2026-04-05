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

    /// Send a synchronous request to ensure the server has processed all
    /// preceding notifications (e.g. `didOpen`). This avoids flaky
    /// fixed-duration sleeps in tests.
    async fn sync_after_open(&mut self, uri: &str) {
        let _ = self
            .request(
                "textDocument/documentSymbol",
                Some(json!({ "textDocument": { "uri": uri } })),
            )
            .await;
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

    // Formatting capabilities
    assert!(
        caps["documentFormattingProvider"]
            .as_bool()
            .unwrap_or(false),
        "expected documentFormattingProvider to be true"
    );
    assert!(
        caps["documentRangeFormattingProvider"]
            .as_bool()
            .unwrap_or(false),
        "expected documentRangeFormattingProvider to be true"
    );

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
async fn test_formatting_reformats_unformatted_document() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    // Badly formatted circom: inconsistent spacing, bad indentation
    let unformatted =
        "pragma circom 2.0.0;\ntemplate Foo(n){\nsignal input a;\nsignal output b;\nb<==a;\n}\n";

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": "file:///test/format.circom",
                    "languageId": "circom",
                    "version": 1,
                    "text": unformatted
                }
            })),
        )
        .await;

    client.sync_after_open("file:///test/format.circom").await;

    let resp = client
        .request(
            "textDocument/formatting",
            Some(json!({
                "textDocument": { "uri": "file:///test/format.circom" },
                "options": { "tabSize": 4, "insertSpaces": true }
            })),
        )
        .await;

    let edits = resp["result"].as_array().expect("expected array of edits");
    assert!(!edits.is_empty(), "expected at least one text edit");

    // Apply the edits and verify the result is well-formatted
    let new_text = apply_edits(unformatted, edits);
    assert!(
        new_text.contains("    signal input a;"),
        "expected proper indentation in formatted output, got:\n{new_text}"
    );
    assert!(
        new_text.contains("    b <== a;"),
        "expected proper operator spacing in formatted output, got:\n{new_text}"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_formatting_is_idempotent() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    // Already well-formatted circom
    let formatted = "pragma circom 2.0.0;\n\ntemplate Foo(n) {\n    signal input a;\n    signal output b;\n    b <== a;\n}\n";

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": "file:///test/idempotent.circom",
                    "languageId": "circom",
                    "version": 1,
                    "text": formatted
                }
            })),
        )
        .await;

    client
        .sync_after_open("file:///test/idempotent.circom")
        .await;

    let resp = client
        .request(
            "textDocument/formatting",
            Some(json!({
                "textDocument": { "uri": "file:///test/idempotent.circom" },
                "options": { "tabSize": 4, "insertSpaces": true }
            })),
        )
        .await;

    // When already formatted, should return empty edits (no changes needed).
    let edits = resp["result"]
        .as_array()
        .expect("expected array result for idempotent formatting");
    assert!(
        edits.is_empty(),
        "already-formatted code should produce no edits, got: {edits:?}"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_range_formatting_only_changes_selection() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    // The pragma is fine, but the template is badly formatted
    let text = "pragma circom 2.0.0;\n\ntemplate Foo(n){\nsignal input a;\nb<==a;\n}\n";

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": "file:///test/range.circom",
                    "languageId": "circom",
                    "version": 1,
                    "text": text
                }
            })),
        )
        .await;

    client.sync_after_open("file:///test/range.circom").await;

    // Format only the template (lines 2-5, covering "template Foo..." to "}")
    let resp = client
        .request(
            "textDocument/rangeFormatting",
            Some(json!({
                "textDocument": { "uri": "file:///test/range.circom" },
                "range": {
                    "start": { "line": 2, "character": 0 },
                    "end": { "line": 5, "character": 1 }
                },
                "options": { "tabSize": 4, "insertSpaces": true }
            })),
        )
        .await;

    let edits = resp["result"].as_array().expect("expected array of edits");
    assert!(!edits.is_empty(), "expected at least one text edit");

    // Verify edits don't touch lines outside the requested range
    for edit in edits {
        let start_line = edit["range"]["start"]["line"].as_u64().unwrap();
        let end_line = edit["range"]["end"]["line"].as_u64().unwrap();
        assert!(
            start_line >= 2,
            "range formatting should not modify lines before the requested range, got edit starting at line {start_line}"
        );
        assert!(
            end_line <= 5,
            "range formatting should not modify lines after the requested range, got edit ending at line {end_line}"
        );
    }

    // Apply edits and verify the formatted content is correct
    let formatted = apply_edits(text, edits);
    assert!(
        formatted.contains("template Foo(n) {"),
        "expected properly formatted template header, got: {formatted}"
    );
    assert!(
        formatted.contains("    signal input a;"),
        "expected indented signal declaration, got: {formatted}"
    );
    // Pragma line should be untouched
    assert!(
        formatted.starts_with("pragma circom 2.0.0;\n"),
        "pragma line should be unchanged, got: {formatted}"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_formatting_does_not_crash_on_comments() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    let text = "pragma circom 2.0.0;\n// A template\ntemplate Foo(){\nsignal input a;\n}\n";

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": "file:///test/comments.circom",
                    "languageId": "circom",
                    "version": 1,
                    "text": text
                }
            })),
        )
        .await;

    client.sync_after_open("file:///test/comments.circom").await;

    let resp = client
        .request(
            "textDocument/formatting",
            Some(json!({
                "textDocument": { "uri": "file:///test/comments.circom" },
                "options": { "tabSize": 4, "insertSpaces": true }
            })),
        )
        .await;

    assert!(
        resp.get("error").is_none(),
        "formatting should not return an error: {resp}"
    );

    // Formatting should succeed and preserve comments.
    let result = &resp["result"];
    assert!(
        result.is_array(),
        "expected formatting edits for file with comments, got: {result}"
    );

    // Apply the edits and verify comments are preserved.
    let edits = result.as_array().unwrap();
    assert!(
        !edits.is_empty(),
        "expected non-empty edits for file with comments (unformatted input should produce edits)"
    );
    let new_text = apply_edits(text, edits);
    assert!(
        new_text.contains("// A template"),
        "formatted output should preserve comments: {new_text}"
    );
    assert!(
        new_text.contains("signal input a"),
        "formatted output should preserve code: {new_text}"
    );

    client.shutdown_and_exit().await;
}

/// Apply LSP text edits to source text, returning the result.
/// Edits are applied in reverse order to preserve earlier positions.
///
/// **Note:** LSP `character` offsets are UTF-16 code units, but this helper
/// treats them as byte offsets. This is correct for ASCII-only test inputs;
/// non-ASCII sources would require proper UTF-16 offset conversion.
fn apply_edits(text: &str, edits: &[Value]) -> String {
    debug_assert!(
        text.is_ascii(),
        "apply_edits assumes ASCII; non-ASCII input would produce wrong offsets"
    );
    let mut sorted: Vec<&Value> = edits.iter().collect();
    sorted.sort_by(|a, b| {
        let a_line = a["range"]["start"]["line"].as_u64().unwrap();
        let b_line = b["range"]["start"]["line"].as_u64().unwrap();
        let a_char = a["range"]["start"]["character"].as_u64().unwrap();
        let b_char = b["range"]["start"]["character"].as_u64().unwrap();
        b_line.cmp(&a_line).then_with(|| b_char.cmp(&a_char))
    });

    let mut result = text.to_string();
    for edit in &sorted {
        let start_line = edit["range"]["start"]["line"].as_u64().unwrap() as usize;
        let start_char = edit["range"]["start"]["character"].as_u64().unwrap() as usize;
        let end_line = edit["range"]["end"]["line"].as_u64().unwrap() as usize;
        let end_char = edit["range"]["end"]["character"].as_u64().unwrap() as usize;
        let new_text = edit["newText"].as_str().unwrap();

        let mut offset = 0;
        let res_lines: Vec<&str> = result.split('\n').collect();
        let mut start_offset = 0;
        let mut end_offset = 0;
        for (i, line) in res_lines.iter().enumerate() {
            if i == start_line {
                start_offset = offset + start_char;
            }
            if i == end_line {
                end_offset = offset + end_char;
                break;
            }
            offset += line.len() + 1;
        }

        result = format!(
            "{}{}{}",
            &result[..start_offset],
            new_text,
            &result[end_offset..]
        );
    }

    result
}

#[tokio::test]
async fn test_range_formatting_no_spurious_newlines() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    let text = "pragma circom 2.0.0;\n\ntemplate Foo(n){\nsignal input a;\nb<==a;\n}\n";

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": "file:///test/newline_check.circom",
                    "languageId": "circom",
                    "version": 1,
                    "text": text
                }
            })),
        )
        .await;

    client
        .sync_after_open("file:///test/newline_check.circom")
        .await;

    let resp = client
        .request(
            "textDocument/rangeFormatting",
            Some(json!({
                "textDocument": { "uri": "file:///test/newline_check.circom" },
                "range": {
                    "start": { "line": 2, "character": 0 },
                    "end": { "line": 5, "character": 1 }
                },
                "options": { "tabSize": 4, "insertSpaces": true }
            })),
        )
        .await;

    let edits = resp["result"].as_array().expect("expected array of edits");
    assert!(!edits.is_empty(), "expected at least one text edit");

    let result = apply_edits(text, edits);
    assert!(
        !result.contains("\n\n\n"),
        "range formatting produced spurious blank lines:\n{result}"
    );

    assert!(
        result.starts_with("pragma circom 2.0.0;\n"),
        "pragma line was modified:\n{result}"
    );

    client.shutdown_and_exit().await;
}

#[tokio::test]
async fn test_formatting_returns_none_on_parse_errors() {
    let mut client = LspClient::spawn().await;
    client.initialize().await;

    // File with a syntax error (missing semicolon after pragma)
    let text = "pragma circom 2.0.0\ntemplate Foo() {\n    signal input a;\n}\n";

    client
        .notify(
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": "file:///test/parse_error.circom",
                    "languageId": "circom",
                    "version": 1,
                    "text": text
                }
            })),
        )
        .await;

    client
        .sync_after_open("file:///test/parse_error.circom")
        .await;

    let resp = client
        .request(
            "textDocument/formatting",
            Some(json!({
                "textDocument": { "uri": "file:///test/parse_error.circom" },
                "options": { "tabSize": 4, "insertSpaces": true }
            })),
        )
        .await;

    assert!(
        resp.get("error").is_none(),
        "formatting should not return a JSON-RPC error: {resp}"
    );

    // The result should be null (no edits) because the file has parse errors
    let result = &resp["result"];
    assert!(
        result.is_null(),
        "formatting a file with parse errors should return null, got: {result}"
    );

    client.shutdown_and_exit().await;
}
