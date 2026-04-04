use dashmap::DashMap;
use ropey::Rope;
use tower_lsp::lsp_types::Url;

/// Stores open document state for the LSP server.
pub struct DocumentStore {
    documents: DashMap<Url, DocumentState>,
}

/// State for a single open document.
pub struct DocumentState {
    /// The current content of the document.
    pub content: Rope,
    /// The version number from the client.
    pub version: i32,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            documents: DashMap::new(),
        }
    }

    /// Open a new document.
    pub fn open(&self, uri: Url, version: i32, text: String) {
        self.documents.insert(
            uri,
            DocumentState {
                content: Rope::from_str(&text),
                version,
            },
        );
    }

    /// Apply incremental changes to a document.
    pub fn apply_changes(
        &self,
        uri: &Url,
        version: i32,
        changes: Vec<tower_lsp::lsp_types::TextDocumentContentChangeEvent>,
    ) {
        if let Some(mut doc) = self.documents.get_mut(uri) {
            for change in changes {
                if let Some(range) = change.range {
                    let start = position_to_offset(&doc.content, range.start);
                    let end = position_to_offset(&doc.content, range.end);
                    doc.content.remove(start..end);
                    doc.content.insert(start, &change.text);
                } else {
                    // Full document replacement.
                    doc.content = Rope::from_str(&change.text);
                }
            }
            doc.version = version;
        }
    }

    /// Close a document, removing it from the store.
    pub fn close(&self, uri: &Url) {
        self.documents.remove(uri);
    }

    /// Get the full text of a document.
    pub fn get_text(&self, uri: &Url) -> Option<String> {
        self.documents.get(uri).map(|doc| doc.content.to_string())
    }

    /// Get the version of a document.
    pub fn get_version(&self, uri: &Url) -> Option<i32> {
        self.documents.get(uri).map(|doc| doc.version)
    }

    /// Check if a document is open.
    pub fn is_open(&self, uri: &Url) -> bool {
        self.documents.contains_key(uri)
    }
}

impl Default for DocumentStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert an LSP Position to a byte offset in a Rope.
fn position_to_offset(rope: &Rope, position: tower_lsp::lsp_types::Position) -> usize {
    let line = position.line as usize;
    if line >= rope.len_lines() {
        return rope.len_chars();
    }
    let line_start = rope.line_to_char(line);
    let col = position.character as usize;
    let line_len = rope.line(line).len_chars();
    line_start + col.min(line_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_uri(name: &str) -> Url {
        Url::parse(&format!("file:///test/{name}")).unwrap()
    }

    #[test]
    fn open_and_get_text() {
        let store = DocumentStore::new();
        let uri = test_uri("main.circom");
        store.open(uri.clone(), 1, "pragma circom \"2.2.3\";".to_string());

        assert_eq!(
            store.get_text(&uri),
            Some("pragma circom \"2.2.3\";".to_string())
        );
        assert_eq!(store.get_version(&uri), Some(1));
        assert!(store.is_open(&uri));
    }

    #[test]
    fn close_removes_document() {
        let store = DocumentStore::new();
        let uri = test_uri("main.circom");
        store.open(uri.clone(), 1, "hello".to_string());
        store.close(&uri);

        assert!(!store.is_open(&uri));
        assert_eq!(store.get_text(&uri), None);
    }

    #[test]
    fn full_document_change() {
        let store = DocumentStore::new();
        let uri = test_uri("main.circom");
        store.open(uri.clone(), 1, "old content".to_string());

        store.apply_changes(
            &uri,
            2,
            vec![tower_lsp::lsp_types::TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "new content".to_string(),
            }],
        );

        assert_eq!(store.get_text(&uri), Some("new content".to_string()));
        assert_eq!(store.get_version(&uri), Some(2));
    }

    #[test]
    fn incremental_change() {
        let store = DocumentStore::new();
        let uri = test_uri("main.circom");
        store.open(uri.clone(), 1, "hello world".to_string());

        // Replace "world" (chars 6..11) with "circom"
        store.apply_changes(
            &uri,
            2,
            vec![tower_lsp::lsp_types::TextDocumentContentChangeEvent {
                range: Some(tower_lsp::lsp_types::Range {
                    start: tower_lsp::lsp_types::Position {
                        line: 0,
                        character: 6,
                    },
                    end: tower_lsp::lsp_types::Position {
                        line: 0,
                        character: 11,
                    },
                }),
                range_length: None,
                text: "circom".to_string(),
            }],
        );

        assert_eq!(store.get_text(&uri), Some("hello circom".to_string()));
    }

    #[test]
    fn incremental_change_multiline() {
        let store = DocumentStore::new();
        let uri = test_uri("main.circom");
        store.open(uri.clone(), 1, "line1\nline2\nline3".to_string());

        // Replace "line2" on the second line
        store.apply_changes(
            &uri,
            2,
            vec![tower_lsp::lsp_types::TextDocumentContentChangeEvent {
                range: Some(tower_lsp::lsp_types::Range {
                    start: tower_lsp::lsp_types::Position {
                        line: 1,
                        character: 0,
                    },
                    end: tower_lsp::lsp_types::Position {
                        line: 1,
                        character: 5,
                    },
                }),
                range_length: None,
                text: "replaced".to_string(),
            }],
        );

        assert_eq!(
            store.get_text(&uri),
            Some("line1\nreplaced\nline3".to_string())
        );
    }

    #[test]
    fn change_to_unopened_document_is_noop() {
        let store = DocumentStore::new();
        let uri = test_uri("nonexistent.circom");

        store.apply_changes(
            &uri,
            1,
            vec![tower_lsp::lsp_types::TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "content".to_string(),
            }],
        );

        assert!(!store.is_open(&uri));
    }
}
