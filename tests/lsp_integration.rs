//! End-to-end LSP integration tests driven by fixtures.
//!
//! Each fixture pair (`foo.circom` + `foo.expected.json`) describes a
//! single LSP round-trip. The JSON file specifies which capability to
//! exercise and what to assert on the response. The test harness spawns
//! an in-process `CinccinoBackend`, opens the fixture, dispatches the
//! request, and validates the response.
//!
//! Supported capabilities per fixture:
//! - `diagnostics` — wait for `textDocument/publishDiagnostics`
//! - `hover` — `textDocument/hover` at a position
//! - `definition` — `textDocument/definition` at a position
//! - `references` — `textDocument/references` at a position
//! - `completion` — `textDocument/completion` at a position
//! - `document_symbol` — `textDocument/documentSymbol`
//! - `signature_help` — `textDocument/signatureHelp` at a position

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cinccino::server::CinccinoBackend;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::time::timeout;
use tower_lsp::{LspService, Server};

// ─── in-process LSP client ─────────────────────────────────────────────

struct InProcessClient {
    reader: BufReader<DuplexStream>,
    writer: DuplexStream,
    next_id: i64,
}

impl InProcessClient {
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
        self.send(&msg).await;
        self.recv_response(id).await
    }

    async fn notify(&mut self, method: &str, params: Option<Value>) {
        let mut msg = json!({ "jsonrpc": "2.0", "method": method });
        if let Some(p) = params {
            msg["params"] = p;
        }
        self.send(&msg).await;
    }

    async fn send(&mut self, msg: &Value) {
        let body = serde_json::to_string(msg).unwrap();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.writer.write_all(header.as_bytes()).await.unwrap();
        self.writer.write_all(body.as_bytes()).await.unwrap();
        self.writer.flush().await.unwrap();
    }

    async fn recv_response(&mut self, id: i64) -> Value {
        loop {
            let msg = self.recv().await;
            if msg.get("id").and_then(|i| i.as_i64()) == Some(id) {
                return msg;
            }
            // Auto-reply to server→client requests so the server doesn't block.
            if msg.get("method").is_some() && msg.get("id").is_some() {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": msg["id"].clone(),
                    "result": null
                });
                self.send(&resp).await;
            }
        }
    }

    async fn recv(&mut self) -> Value {
        timeout(Duration::from_secs(10), async {
            let mut content_length: usize = 0;
            loop {
                let mut line = String::new();
                self.reader.read_line(&mut line).await.unwrap();
                let line = line.trim();
                if line.is_empty() {
                    break;
                }
                if let Some(v) = line.strip_prefix("Content-Length: ") {
                    content_length = v.parse().unwrap();
                }
            }
            assert!(content_length > 0);
            let mut buf = vec![0u8; content_length];
            tokio::io::AsyncReadExt::read_exact(&mut self.reader, &mut buf)
                .await
                .unwrap();
            serde_json::from_slice::<Value>(&buf).unwrap()
        })
        .await
        .expect("lsp read timed out")
    }

    async fn wait_for_notification(&mut self, method: &str) -> Value {
        timeout(Duration::from_secs(5), async {
            loop {
                let msg = self.recv().await;
                if msg.get("method").and_then(|m| m.as_str()) == Some(method) {
                    return msg;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {method}"))
    }

    async fn initialize(&mut self) {
        let _ = self
            .request(
                "initialize",
                Some(json!({ "processId": null, "capabilities": {}, "rootUri": null })),
            )
            .await;
        self.notify("initialized", Some(json!({}))).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    async fn open(&mut self, uri: &str, text: &str) {
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
    }

    async fn shutdown(&mut self) {
        let _ = self.request("shutdown", None).await;
        self.notify("exit", None).await;
    }
}

// ─── fixture loading ───────────────────────────────────────────────────

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("lsp")
}

fn load_fixture(stem: &str) -> (String, Value) {
    let base = fixture_dir();
    let circom_path = base.join(format!("{stem}.circom"));
    let json_path = base.join(format!("{stem}.expected.json"));
    let text = fs::read_to_string(&circom_path)
        .unwrap_or_else(|_| panic!("failed to read {circom_path:?}"));
    let json_raw =
        fs::read_to_string(&json_path).unwrap_or_else(|_| panic!("failed to read {json_path:?}"));
    let expected: Value = serde_json::from_str(&json_raw).expect("invalid fixture JSON");
    (text, expected)
}

fn uri_for(stem: &str) -> String {
    format!("file:///{}.circom", stem)
}

fn position(v: &Value) -> Value {
    json!({
        "line": v["line"].as_u64().unwrap(),
        "character": v["character"].as_u64().unwrap(),
    })
}

// ─── per-capability checks ─────────────────────────────────────────────

async fn check_diagnostics(stem: &str) {
    let (text, expected) = load_fixture(stem);
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    let uri = uri_for(stem);
    client.open(&uri, &text).await;

    let msg = client
        .wait_for_notification("textDocument/publishDiagnostics")
        .await;
    let diags = msg["params"]["diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let spec = &expected["diagnostics"];
    if let Some(n) = spec.get("count").and_then(|v| v.as_u64()) {
        assert_eq!(
            diags.len() as u64,
            n,
            "diagnostic count mismatch: {diags:?}"
        );
    }
    if let Some(n) = spec.get("min_count").and_then(|v| v.as_u64()) {
        assert!(
            (diags.len() as u64) >= n,
            "expected >= {n} diagnostics, got {}: {diags:?}",
            diags.len()
        );
    }
    if let Some(needles) = spec.get("must_contain").and_then(|v| v.as_array()) {
        for needle in needles {
            let n = needle.as_str().unwrap();
            let found = diags
                .iter()
                .any(|d| d["message"].as_str().unwrap_or("").contains(n));
            assert!(found, "no diagnostic contains {n:?}: {diags:?}");
        }
    }
    client.shutdown().await;
}

async fn check_hover(stem: &str) {
    let (text, expected) = load_fixture(stem);
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    let uri = uri_for(stem);
    client.open(&uri, &text).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let spec = &expected["hover"];
    let resp = client
        .request(
            "textDocument/hover",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": position(&spec["position"]),
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected hover result, got null");
    let value = result["contents"]["value"].as_str().unwrap_or("");
    let needle = spec["must_contain"].as_str().unwrap();
    assert!(value.contains(needle), "hover {value:?} missing {needle:?}");

    client.shutdown().await;
}

async fn check_definition(stem: &str) {
    let (text, expected) = load_fixture(stem);
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    let uri = uri_for(stem);
    client.open(&uri, &text).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let spec = &expected["definition"];
    let resp = client
        .request(
            "textDocument/definition",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": position(&spec["position"]),
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected definition, got null");
    assert_eq!(result["uri"].as_str().unwrap(), uri, "uri mismatch");
    let expected_line = spec["target_line"].as_u64().unwrap();
    assert_eq!(
        result["range"]["start"]["line"].as_u64().unwrap(),
        expected_line,
        "target line mismatch"
    );
    client.shutdown().await;
}

async fn check_references(stem: &str) {
    let (text, expected) = load_fixture(stem);
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    let uri = uri_for(stem);
    client.open(&uri, &text).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let spec = &expected["references"];
    let include_decl = spec["include_declaration"].as_bool().unwrap_or(true);
    let resp = client
        .request(
            "textDocument/references",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": position(&spec["position"]),
                "context": { "includeDeclaration": include_decl },
            })),
        )
        .await;

    let locs = resp["result"].as_array().cloned().unwrap_or_default();
    if let Some(min) = spec.get("min_count").and_then(|v| v.as_u64()) {
        assert!(
            (locs.len() as u64) >= min,
            "expected >= {min} references, got {}",
            locs.len()
        );
    }
    client.shutdown().await;
}

async fn check_completion(stem: &str) {
    let (text, expected) = load_fixture(stem);
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    let uri = uri_for(stem);
    client.open(&uri, &text).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let spec = &expected["completion"];
    let resp = client
        .request(
            "textDocument/completion",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": position(&spec["position"]),
            })),
        )
        .await;

    let items = resp["result"].as_array().cloned().unwrap_or_default();
    let labels: Vec<String> = items
        .iter()
        .filter_map(|i| i["label"].as_str().map(String::from))
        .collect();
    if let Some(needles) = spec["must_contain"].as_array() {
        for needle in needles {
            let n = needle.as_str().unwrap();
            assert!(
                labels.iter().any(|l| l == n),
                "completions {labels:?} missing {n:?}"
            );
        }
    }
    client.shutdown().await;
}

async fn check_document_symbol(stem: &str) {
    let (text, expected) = load_fixture(stem);
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    let uri = uri_for(stem);
    client.open(&uri, &text).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let spec = &expected["document_symbol"];
    let resp = client
        .request(
            "textDocument/documentSymbol",
            Some(json!({ "textDocument": { "uri": uri } })),
        )
        .await;

    let symbols = resp["result"].as_array().cloned().unwrap_or_default();
    if let Some(expected_names) = spec["top_level_names"].as_array() {
        let names: Vec<&str> = symbols
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        for expected_name in expected_names {
            let n = expected_name.as_str().unwrap();
            assert!(names.contains(&n), "top-level {names:?} missing {n:?}");
        }
    }
    if let Some(expected_kids) = spec.get("children_count").and_then(|v| v.as_u64()) {
        let kids = symbols[0]["children"]
            .as_array()
            .map(|c| c.len())
            .unwrap_or(0);
        assert_eq!(kids as u64, expected_kids, "children_count mismatch");
    }
    client.shutdown().await;
}

async fn check_signature_help(stem: &str) {
    let (text, expected) = load_fixture(stem);
    let mut client = InProcessClient::spawn();
    client.initialize().await;
    let uri = uri_for(stem);
    client.open(&uri, &text).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let spec = &expected["signature_help"];
    let resp = client
        .request(
            "textDocument/signatureHelp",
            Some(json!({
                "textDocument": { "uri": uri },
                "position": position(&spec["position"]),
            })),
        )
        .await;

    let result = &resp["result"];
    assert!(!result.is_null(), "expected signature help, got null");
    let label = result["signatures"][0]["label"].as_str().unwrap();
    let expected_label = spec["signature_label"].as_str().unwrap();
    assert_eq!(label, expected_label, "signature label mismatch");
    if let Some(ap) = spec.get("active_parameter").and_then(|v| v.as_u64()) {
        assert_eq!(
            result["activeParameter"].as_u64().unwrap(),
            ap,
            "activeParameter mismatch"
        );
    }
    client.shutdown().await;
}

// ─── the actual #[test] entry points ───────────────────────────────────

#[tokio::test]
async fn lsp_diagnostics_clean_file() {
    check_diagnostics("diagnostics_ok").await;
}

#[tokio::test]
async fn lsp_diagnostics_assign_to_input() {
    check_diagnostics("diagnostics_err").await;
}

#[tokio::test]
async fn lsp_hover_on_template() {
    check_hover("hover_template").await;
}

#[tokio::test]
async fn lsp_hover_on_signal() {
    check_hover("hover_signal").await;
}

#[tokio::test]
async fn lsp_definition_template() {
    check_definition("definition_template").await;
}

#[tokio::test]
async fn lsp_definition_signal() {
    check_definition("definition_signal").await;
}

#[tokio::test]
async fn lsp_references_signal() {
    check_references("references_signal").await;
}

#[tokio::test]
async fn lsp_completion_top_level() {
    check_completion("completion_top_level").await;
}

#[tokio::test]
async fn lsp_document_symbol_template() {
    check_document_symbol("document_symbol").await;
}

#[tokio::test]
async fn lsp_signature_help_template() {
    check_signature_help("signature_help_template").await;
}

#[tokio::test]
async fn lsp_signature_help_function_active_param() {
    check_signature_help("signature_help_function").await;
}

// ─── smoke test: every fixture has a `.expected.json` sibling ──────────

#[test]
fn fixture_pairs_are_complete() {
    let dir = fixture_dir();
    let mut stems = std::collections::BTreeSet::new();
    for entry in fs::read_dir(&dir).expect("fixture dir missing") {
        let path: &Path = &entry.unwrap().path();
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if path.extension().and_then(|e| e.to_str()) == Some("circom") {
                stems.insert(stem.to_string());
            }
        }
    }
    for stem in &stems {
        let json = dir.join(format!("{stem}.expected.json"));
        assert!(json.exists(), "missing {json:?}");
    }
}
