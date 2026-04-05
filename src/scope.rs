//! Scope tree for Circom symbol resolution.
//!
//! Scopes form a tree: the root is the file scope, and nested scopes
//! (template bodies, function bodies, blocks) are children. Symbol lookup
//! walks from the current scope up to the root.

use crate::symbol::{ScopeId, ScopeKind, SymbolId};
use std::collections::HashMap;

/// A single scope in the scope tree.
#[derive(Debug, Clone)]
pub struct Scope {
    pub id: ScopeId,
    pub kind: ScopeKind,
    pub parent: Option<ScopeId>,
    pub children: Vec<ScopeId>,
    /// Symbols defined directly in this scope, indexed by name.
    symbols: HashMap<String, Vec<SymbolId>>,
}

impl Scope {
    fn new(id: ScopeId, kind: ScopeKind, parent: Option<ScopeId>) -> Self {
        Self {
            id,
            kind,
            parent,
            children: Vec::new(),
            symbols: HashMap::new(),
        }
    }

    /// Insert a symbol name into this scope. Returns `true` if a symbol
    /// with this name already existed (duplicate detection).
    pub fn insert(&mut self, name: &str, id: SymbolId) -> bool {
        let entry = self.symbols.entry(name.to_string()).or_default();
        let is_dup = !entry.is_empty();
        entry.push(id);
        is_dup
    }

    /// Look up a symbol by name in this scope only.
    pub fn lookup_local(&self, name: &str) -> Option<&[SymbolId]> {
        self.symbols.get(name).map(|v| v.as_slice())
    }

    /// Get all symbol IDs in this scope.
    pub fn all_symbols(&self) -> impl Iterator<Item = SymbolId> + '_ {
        self.symbols.values().flat_map(|v| v.iter().copied())
    }

    /// Get all symbol names in this scope.
    pub fn symbol_names(&self) -> impl Iterator<Item = &str> {
        self.symbols.keys().map(|s| s.as_str())
    }
}

/// The scope tree manages all scopes and provides lookup operations.
#[derive(Debug, Clone)]
pub struct ScopeTree {
    scopes: Vec<Scope>,
}

impl ScopeTree {
    pub fn new() -> Self {
        Self { scopes: Vec::new() }
    }

    /// Create a new root scope (file scope).
    pub fn create_root(&mut self, kind: ScopeKind) -> ScopeId {
        let id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(Scope::new(id, kind, None));
        id
    }

    /// Create a child scope under the given parent.
    pub fn create_child(&mut self, parent: ScopeId, kind: ScopeKind) -> ScopeId {
        let id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(Scope::new(id, kind, Some(parent)));
        self.scopes[parent.0 as usize].children.push(id);
        id
    }

    /// Get a scope by ID.
    pub fn get(&self, id: ScopeId) -> &Scope {
        &self.scopes[id.0 as usize]
    }

    /// Get a mutable reference to a scope by ID.
    pub fn get_mut(&mut self, id: ScopeId) -> &mut Scope {
        &mut self.scopes[id.0 as usize]
    }

    /// Insert a symbol into a scope. Returns `true` if duplicate.
    pub fn insert_symbol(&mut self, scope: ScopeId, name: &str, id: SymbolId) -> bool {
        let s = self.get_mut(scope);
        s.insert(name, id)
    }

    /// Look up a symbol by name, walking up the scope chain.
    /// Returns the first matching symbol IDs found.
    pub fn lookup(&self, scope: ScopeId, name: &str) -> Option<(ScopeId, &[SymbolId])> {
        let mut current = Some(scope);
        while let Some(sid) = current {
            let s = self.get(sid);
            if let Some(ids) = s.lookup_local(name) {
                return Some((sid, ids));
            }
            current = s.parent;
        }
        None
    }

    /// Look up a symbol only in the given scope (no parent walk).
    pub fn lookup_local(&self, scope: ScopeId, name: &str) -> Option<&[SymbolId]> {
        self.get(scope).lookup_local(name)
    }

    /// Number of scopes in the tree.
    pub fn len(&self) -> usize {
        self.scopes.len()
    }

    /// Whether the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }

    /// Remove all scopes whose IDs are in the given set, and
    /// remove them as children from their parents.
    /// Used for incremental updates when a file is re-parsed.
    pub fn remove_scopes(&mut self, to_remove: &[ScopeId]) {
        // Mark scopes for removal.
        let remove_set: std::collections::HashSet<ScopeId> = to_remove.iter().copied().collect();

        // Remove from parent's children lists.
        for &sid in to_remove {
            if let Some(parent_id) = self.scopes[sid.0 as usize].parent {
                self.scopes[parent_id.0 as usize]
                    .children
                    .retain(|c| !remove_set.contains(c));
            }
        }

        // Clear the removed scopes. We don't compact to keep ScopeId values
        // stable, which means the Vec grows monotonically over repeated
        // re-index cycles. For a long-running LSP this could accumulate
        // dead entries. A future `compact()` method could reassign IDs and
        // reclaim slots, or we could switch to a SlotMap/arena with reuse.
        for &sid in to_remove {
            let scope = &mut self.scopes[sid.0 as usize];
            scope.symbols.clear();
            scope.children.clear();
        }
    }
}

impl Default for ScopeTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolId;

    #[test]
    fn create_root_scope() {
        let mut tree = ScopeTree::new();
        let root = tree.create_root(ScopeKind::File);
        assert_eq!(root, ScopeId(0));
        assert_eq!(tree.get(root).kind, ScopeKind::File);
        assert!(tree.get(root).parent.is_none());
    }

    #[test]
    fn create_child_scope() {
        let mut tree = ScopeTree::new();
        let root = tree.create_root(ScopeKind::File);
        let child = tree.create_child(root, ScopeKind::Template);
        assert_eq!(child, ScopeId(1));
        assert_eq!(tree.get(child).parent, Some(root));
        assert!(tree.get(root).children.contains(&child));
    }

    #[test]
    fn insert_and_lookup_local() {
        let mut tree = ScopeTree::new();
        let root = tree.create_root(ScopeKind::File);
        tree.insert_symbol(root, "Foo", SymbolId(0));

        let result = tree.lookup_local(root, "Foo");
        assert_eq!(result, Some(&[SymbolId(0)][..]));

        assert!(tree.lookup_local(root, "Bar").is_none());
    }

    #[test]
    fn lookup_walks_up_scope_chain() {
        let mut tree = ScopeTree::new();
        let root = tree.create_root(ScopeKind::File);
        let child = tree.create_child(root, ScopeKind::Template);
        let grandchild = tree.create_child(child, ScopeKind::Block);

        tree.insert_symbol(root, "Foo", SymbolId(0));
        tree.insert_symbol(child, "bar", SymbolId(1));

        // Lookup from grandchild finds "bar" in parent (template scope).
        let (scope, ids) = tree.lookup(grandchild, "bar").unwrap();
        assert_eq!(scope, child);
        assert_eq!(ids, &[SymbolId(1)]);

        // Lookup from grandchild finds "Foo" in root (file scope).
        let (scope, ids) = tree.lookup(grandchild, "Foo").unwrap();
        assert_eq!(scope, root);
        assert_eq!(ids, &[SymbolId(0)]);

        // Lookup of non-existent symbol returns None.
        assert!(tree.lookup(grandchild, "baz").is_none());
    }

    #[test]
    fn inner_scope_shadows_outer() {
        let mut tree = ScopeTree::new();
        let root = tree.create_root(ScopeKind::File);
        let child = tree.create_child(root, ScopeKind::Template);
        let block = tree.create_child(child, ScopeKind::Block);

        // "x" in template scope
        tree.insert_symbol(child, "x", SymbolId(0));
        // "x" also in block scope (shadows the outer one)
        tree.insert_symbol(block, "x", SymbolId(1));

        // Lookup from block finds the inner one.
        let (scope, ids) = tree.lookup(block, "x").unwrap();
        assert_eq!(scope, block);
        assert_eq!(ids, &[SymbolId(1)]);

        // Lookup from template scope finds its own.
        let (scope, ids) = tree.lookup(child, "x").unwrap();
        assert_eq!(scope, child);
        assert_eq!(ids, &[SymbolId(0)]);
    }

    #[test]
    fn duplicate_detection() {
        let mut tree = ScopeTree::new();
        let root = tree.create_root(ScopeKind::File);

        let is_dup = tree.insert_symbol(root, "Foo", SymbolId(0));
        assert!(!is_dup);

        let is_dup = tree.insert_symbol(root, "Foo", SymbolId(1));
        assert!(is_dup);

        // Both are stored.
        let ids = tree.lookup_local(root, "Foo").unwrap();
        assert_eq!(ids, &[SymbolId(0), SymbolId(1)]);
    }

    #[test]
    fn remove_scopes_clears_content() {
        let mut tree = ScopeTree::new();
        let root = tree.create_root(ScopeKind::File);
        let child = tree.create_child(root, ScopeKind::Template);
        tree.insert_symbol(child, "x", SymbolId(0));

        tree.remove_scopes(&[child]);

        // Child's symbols are cleared.
        assert!(tree.lookup_local(child, "x").is_none());
        // Child is removed from parent's children list.
        assert!(!tree.get(root).children.contains(&child));
    }
}
