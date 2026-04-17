//! Completion provider for the Circom LSP.
//!
//! Provides context-aware auto-completion across all Circom constructs:
//! top-level keywords, template/function/bus names, signals, variables,
//! components, dot access (component.signal, bus.field, signal.tag),
//! pragma versions, and include paths.

use tower_lsp::lsp_types::*;

use crate::ast::{self, Item, SignalKind};
use crate::symbol::{ScopeId, ScopeKind, SymbolKind};
use crate::symbol_table::SymbolTable;

/// The context in which completion was triggered.
#[derive(Debug, PartialEq)]
pub enum CompletionContext {
    /// At the top level of a file.
    TopLevel,
    /// Inside a template body.
    TemplateBody(ScopeId),
    /// Inside a function body.
    FunctionBody(ScopeId),
    /// After a dot: the prefix is the text before the dot.
    DotAccess {
        /// The identifier before the dot.
        receiver: String,
        /// The scope where the dot access occurs.
        scope: ScopeId,
    },
    /// Inside a pragma statement.
    Pragma,
    /// Inside an include path.
    Include,
}

/// Top-level keywords available in Circom.
const TOP_LEVEL_KEYWORDS: &[(&str, &str)] = &[
    ("template", "template ${1:Name}(${2:params}) {\n    $0\n}"),
    ("function", "function ${1:name}(${2:params}) {\n    $0\n}"),
    ("bus", "bus ${1:Name}(${2:params}) {\n    $0\n}"),
    ("include", "include \"${1:path}\";"),
    ("pragma", "pragma circom \"${1:2.2.3}\";"),
];

/// Keywords available inside a template body.
const TEMPLATE_BODY_KEYWORDS: &[(&str, &str)] = &[
    ("signal input", "signal input ${1:name};"),
    ("signal output", "signal output ${1:name};"),
    ("signal", "signal ${1:name};"),
    ("var", "var ${1:name};"),
    (
        "component",
        "component ${1:name} = ${2:Template}(${3:args});",
    ),
    ("if", "if (${1:cond}) {\n    $0\n}"),
    (
        "for",
        "for (var ${1:i} = 0; ${1:i} < ${2:n}; ${1:i}++) {\n    $0\n}",
    ),
    ("while", "while (${1:cond}) {\n    $0\n}"),
    ("log", "log(${1:msg});"),
    ("assert", "assert(${1:expr});"),
    ("return", "return ${1:expr};"),
    ("parallel", "parallel"),
];

/// Keywords available inside a function body.
const FUNCTION_BODY_KEYWORDS: &[(&str, &str)] = &[
    ("var", "var ${1:name};"),
    ("if", "if (${1:cond}) {\n    $0\n}"),
    (
        "for",
        "for (var ${1:i} = 0; ${1:i} < ${2:n}; ${1:i}++) {\n    $0\n}",
    ),
    ("while", "while (${1:cond}) {\n    $0\n}"),
    ("log", "log(${1:msg});"),
    ("assert", "assert(${1:expr});"),
    ("return", "return ${1:expr};"),
];

/// Known pragma Circom versions.
const PRAGMA_VERSIONS: &[&str] = &[
    "2.0.0", "2.1.0", "2.1.1", "2.1.2", "2.1.3", "2.1.4", "2.1.5", "2.1.6", "2.1.7", "2.1.8",
    "2.2.0", "2.2.1", "2.2.2", "2.2.3",
];

/// Determine the completion context from the cursor position and parsed AST.
pub fn detect_context(
    source: &str,
    offset: usize,
    ast: &crate::ast::File,
    table: &SymbolTable,
    file_path: &str,
) -> CompletionContext {
    let before = &source[..offset.min(source.len())];

    // Check if we're inside a pragma statement (not yet terminated).
    if is_in_pragma(before) {
        return CompletionContext::Pragma;
    }

    // Check if we're inside an include path.
    if is_in_include(before) {
        return CompletionContext::Include;
    }

    // Check if there's a dot access (e.g. `comp.` or `bus.`).
    if let Some(receiver) = detect_dot_receiver(before) {
        let scope = find_scope_at_offset_ast(ast, offset, table, file_path);
        return CompletionContext::DotAccess { receiver, scope };
    }

    // Use AST-based scope detection.
    let scope = find_scope_at_offset_ast(ast, offset, table, file_path);
    let scope_kind = &table.scopes.get(scope).kind;

    match scope_kind {
        ScopeKind::File => CompletionContext::TopLevel,
        ScopeKind::Template => CompletionContext::TemplateBody(scope),
        ScopeKind::Function => CompletionContext::FunctionBody(scope),
        ScopeKind::Block => {
            let ctx_scope = find_enclosing_body_scope(table, scope);
            match &table.scopes.get(ctx_scope).kind {
                ScopeKind::Template => CompletionContext::TemplateBody(scope),
                ScopeKind::Function => CompletionContext::FunctionBody(scope),
                _ => CompletionContext::TopLevel,
            }
        }
        ScopeKind::Bus => CompletionContext::TopLevel,
    }
}

/// Generate completion items for the given context.
pub fn completions(
    context: &CompletionContext,
    table: &SymbolTable,
    file_path: &str,
) -> Vec<CompletionItem> {
    match context {
        CompletionContext::TopLevel => top_level_completions(table, file_path),
        CompletionContext::TemplateBody(scope) => {
            template_body_completions(table, file_path, *scope)
        }
        CompletionContext::FunctionBody(scope) => {
            function_body_completions(table, file_path, *scope)
        }
        CompletionContext::DotAccess { receiver, scope } => {
            dot_completions(table, file_path, receiver, *scope)
        }
        CompletionContext::Pragma => pragma_completions(),
        // TODO: implement include path completion (#52)
        CompletionContext::Include => Vec::new(),
    }
}

fn top_level_completions(table: &SymbolTable, file_path: &str) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = TOP_LEVEL_KEYWORDS
        .iter()
        .enumerate()
        .map(|(i, (label, snippet))| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            insert_text: Some(snippet.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some(format!("0_{i:03}")),
            ..Default::default()
        })
        .collect();

    add_file_scope_symbols(table, file_path, &mut items);
    items
}

fn template_body_completions(
    table: &SymbolTable,
    file_path: &str,
    scope: ScopeId,
) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = TEMPLATE_BODY_KEYWORDS
        .iter()
        .enumerate()
        .map(|(i, (label, snippet))| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            insert_text: Some(snippet.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some(format!("1_{i:03}")),
            ..Default::default()
        })
        .collect();

    add_scope_symbols(table, file_path, scope, &mut items);
    items
}

fn function_body_completions(
    table: &SymbolTable,
    file_path: &str,
    scope: ScopeId,
) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = FUNCTION_BODY_KEYWORDS
        .iter()
        .enumerate()
        .map(|(i, (label, snippet))| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            insert_text: Some(snippet.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some(format!("1_{i:03}")),
            ..Default::default()
        })
        .collect();

    add_scope_symbols(table, file_path, scope, &mut items);
    items
}

fn component_dot_completions(
    table: &SymbolTable,
    file_path: &str,
    scope: ScopeId,
    template_name: Option<&String>,
) -> Vec<CompletionItem> {
    let Some(tmpl_name) = template_name else {
        return Vec::new();
    };
    let Some(tmpl) = table.lookup_with_includes(scope, tmpl_name, file_path) else {
        return Vec::new();
    };
    match &tmpl.kind {
        SymbolKind::Template(t) => scope_members_as_completions(table, t.body_scope, |kind| {
            matches!(kind, SymbolKind::Signal(_))
        }),
        _ => Vec::new(),
    }
}

fn signal_dot_completions(
    table: &SymbolTable,
    file_path: &str,
    scope: ScopeId,
    sig: &crate::symbol::SignalSymbol,
) -> Vec<CompletionItem> {
    if let Some(bus_name) = &sig.bus_type {
        let Some(bus) = table.lookup_with_includes(scope, bus_name, file_path) else {
            return Vec::new();
        };
        match &bus.kind {
            SymbolKind::Bus(b) => scope_members_as_completions(table, b.body_scope, |_| true),
            _ => Vec::new(),
        }
    } else {
        sig.tags
            .iter()
            .enumerate()
            .map(|(i, tag)| CompletionItem {
                label: tag.clone(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some("tag".to_string()),
                sort_text: Some(format!("0_{i:03}")),
                ..Default::default()
            })
            .collect()
    }
}

fn dot_completions(
    table: &SymbolTable,
    file_path: &str,
    receiver: &str,
    scope: ScopeId,
) -> Vec<CompletionItem> {
    let Some(sym) = table.lookup_with_includes(scope, receiver, file_path) else {
        return Vec::new();
    };

    match &sym.kind {
        SymbolKind::Component(comp) => {
            component_dot_completions(table, file_path, scope, comp.template_name.as_ref())
        }
        SymbolKind::Signal(sig) => signal_dot_completions(table, file_path, scope, sig),
        _ => Vec::new(),
    }
}

fn pragma_completions() -> Vec<CompletionItem> {
    PRAGMA_VERSIONS
        .iter()
        .rev()
        .enumerate()
        .map(|(i, ver)| CompletionItem {
            label: ver.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            sort_text: Some(format!("0_{i:03}")),
            ..Default::default()
        })
        .collect()
}

// ── Helpers ────────────────────────────────────────────────────────

fn add_file_scope_symbols(table: &SymbolTable, file_path: &str, items: &mut Vec<CompletionItem>) {
    let base = items.len();
    if let Some(scope_id) = table.file_scope(file_path) {
        add_scope_local_symbols(table, scope_id, items, base);
    }

    if let Some(includes) = table.includes(file_path) {
        let includes: Vec<String> = includes.to_vec();
        for inc_path in &includes {
            if let Some(scope_id) = table.file_scope(inc_path) {
                let base = items.len();
                add_scope_local_symbols(table, scope_id, items, base);
            }
        }
    }
}

fn add_scope_symbols(
    table: &SymbolTable,
    file_path: &str,
    scope: ScopeId,
    items: &mut Vec<CompletionItem>,
) {
    let mut seen = std::collections::HashSet::new();
    let mut current = Some(scope);
    let base = items.len();

    while let Some(sid) = current {
        let s = table.scopes.get(sid);
        for name in s.symbol_names() {
            if seen.insert(name.to_string()) {
                if let Some(ids) = s.lookup_local(name) {
                    let sym = &table.all_symbols()[ids[0].0 as usize];
                    items.push(symbol_to_completion(sym, items.len() - base));
                }
            }
        }
        current = s.parent;
    }

    // Add file-level and included symbols (dedup with `seen`).
    if let Some(scope_id) = table.file_scope(file_path) {
        let s = table.scopes.get(scope_id);
        for name in s.symbol_names() {
            if seen.insert(name.to_string()) {
                if let Some(ids) = s.lookup_local(name) {
                    let sym = &table.all_symbols()[ids[0].0 as usize];
                    items.push(symbol_to_completion(sym, items.len() - base));
                }
            }
        }
    }
    if let Some(includes) = table.includes(file_path) {
        let includes: Vec<String> = includes.to_vec();
        for inc_path in &includes {
            if let Some(scope_id) = table.file_scope(inc_path) {
                let s = table.scopes.get(scope_id);
                for name in s.symbol_names() {
                    if seen.insert(name.to_string()) {
                        if let Some(ids) = s.lookup_local(name) {
                            let sym = &table.all_symbols()[ids[0].0 as usize];
                            items.push(symbol_to_completion(sym, items.len() - base));
                        }
                    }
                }
            }
        }
    }
}

fn add_scope_local_symbols(
    table: &SymbolTable,
    scope_id: ScopeId,
    items: &mut Vec<CompletionItem>,
    base: usize,
) {
    let scope = table.scopes.get(scope_id);
    for name in scope.symbol_names() {
        if let Some(ids) = scope.lookup_local(name) {
            let sym = &table.all_symbols()[ids[0].0 as usize];
            items.push(symbol_to_completion(sym, items.len() - base));
        }
    }
}

fn param_snippet(params: &[String]) -> String {
    if params.is_empty() {
        "()".to_string()
    } else {
        let params_snippet: Vec<String> = params
            .iter()
            .enumerate()
            .map(|(i, _)| format!("${{{}}}", i + 1))
            .collect();
        format!("({})", params_snippet.join(", "))
    }
}

type CompletionBits = (
    CompletionItemKind,
    String,
    Option<String>,
    Option<InsertTextFormat>,
);

fn symbol_completion_bits(sym: &crate::symbol::Symbol) -> CompletionBits {
    match &sym.kind {
        SymbolKind::Template(t) => (
            CompletionItemKind::CLASS,
            format!("template({})", t.params.join(", ")),
            Some(format!("{}{}", sym.name, param_snippet(&t.params))),
            Some(InsertTextFormat::SNIPPET),
        ),
        SymbolKind::Function(f) => (
            CompletionItemKind::FUNCTION,
            format!("function({})", f.params.join(", ")),
            Some(format!("{}{}", sym.name, param_snippet(&f.params))),
            Some(InsertTextFormat::SNIPPET),
        ),
        SymbolKind::Bus(b) => (
            CompletionItemKind::STRUCT,
            format!("bus({})", b.params.join(", ")),
            None,
            None,
        ),
        SymbolKind::Signal(s) => {
            let dir = match s.kind {
                SignalKind::Input => "input",
                SignalKind::Output => "output",
                SignalKind::Intermediate => "intermediate",
            };
            (
                CompletionItemKind::FIELD,
                format!("signal {dir}"),
                None,
                None,
            )
        }
        SymbolKind::Variable => (CompletionItemKind::VARIABLE, "var".to_string(), None, None),
        SymbolKind::Component(c) => {
            let detail = match &c.template_name {
                Some(n) => format!("component: {n}"),
                None => "component".to_string(),
            };
            (CompletionItemKind::MODULE, detail, None, None)
        }
        SymbolKind::Parameter => (
            CompletionItemKind::VARIABLE,
            "parameter".to_string(),
            None,
            None,
        ),
    }
}

fn symbol_to_completion(sym: &crate::symbol::Symbol, index: usize) -> CompletionItem {
    let (kind, detail, insert_text, insert_text_format) = symbol_completion_bits(sym);

    CompletionItem {
        label: sym.name.clone(),
        kind: Some(kind),
        detail: Some(detail),
        insert_text,
        insert_text_format,
        sort_text: Some(format!("2_{index:03}")),
        ..Default::default()
    }
}

fn scope_members_as_completions(
    table: &SymbolTable,
    scope: ScopeId,
    filter: impl Fn(&SymbolKind) -> bool,
) -> Vec<CompletionItem> {
    let s = table.scopes.get(scope);
    let mut items = Vec::new();
    for name in s.symbol_names() {
        if let Some(ids) = s.lookup_local(name) {
            let sym = &table.all_symbols()[ids[0].0 as usize];
            if filter(&sym.kind) {
                items.push(symbol_to_completion(sym, items.len()));
            }
        }
    }
    items
}

/// Check if the cursor is inside an incomplete pragma statement.
fn is_in_pragma(before: &str) -> bool {
    let line = current_line(before);
    let trimmed = line.trim_start();
    // Only trigger if the line starts with `pragma` and doesn't end with `;`.
    trimmed.starts_with("pragma") && !trimmed.contains(';')
}

/// Check if the cursor is inside an include path.
fn is_in_include(before: &str) -> bool {
    let line = current_line(before);
    let trimmed = line.trim_start();
    trimmed.starts_with("include") && !trimmed.contains(';')
}

/// Get the current line (text after the last newline).
fn current_line(before: &str) -> &str {
    match before.rfind('\n') {
        Some(pos) => &before[pos + 1..],
        None => before,
    }
}

/// Detect if the character before the cursor is a dot, and extract the
/// identifier before it.
fn detect_dot_receiver(before: &str) -> Option<String> {
    let trimmed = before.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
    if let Some(before_dot) = trimmed.strip_suffix('.') {
        let ident: String = before_dot
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if !ident.is_empty() {
            Some(ident)
        } else {
            None
        }
    } else {
        None
    }
}

/// Find the scope at the given byte offset using the parsed AST spans.
///
/// This is more reliable than walking the symbol table's scope tree because
/// AST items carry the full span (including body), whereas symbol spans only
/// cover the name.
pub(crate) fn find_scope_at_offset_ast(
    ast: &crate::ast::File,
    offset: usize,
    table: &SymbolTable,
    file_path: &str,
) -> ScopeId {
    let file_scope = match table.file_scope(file_path) {
        Some(s) => s,
        None => return ScopeId(0),
    };

    // Find which top-level item contains the offset.
    for item in &ast.items {
        match item {
            Item::TemplateDef(t) if t.span.start <= offset && offset <= t.span.end => {
                // Find the template's body scope via the symbol table.
                if let Some(sym) = table.lookup(file_scope, &t.name.name) {
                    if let SymbolKind::Template(ts) = &sym.kind {
                        // Check if we're inside a nested block scope.
                        return find_deepest_block_scope(table, ts.body_scope, &t.body, offset);
                    }
                }
            }
            Item::FunctionDef(f) if f.span.start <= offset && offset <= f.span.end => {
                if let Some(sym) = table.lookup(file_scope, &f.name.name) {
                    if let SymbolKind::Function(fs) = &sym.kind {
                        return find_deepest_block_scope(table, fs.body_scope, &f.body, offset);
                    }
                }
            }
            Item::BusDef(b) if b.span.start <= offset && offset <= b.span.end => {
                if let Some(sym) = table.lookup(file_scope, &b.name.name) {
                    if let SymbolKind::Bus(bs) = &sym.kind {
                        return bs.body_scope;
                    }
                }
            }
            _ => {}
        }
    }

    file_scope
}

/// Find the deepest block scope within a template/function body that
/// contains the offset.
///
/// Child block scopes are created in source order (one per `Block`,
/// `For`, `While`, and one or two per `IfElse`). We track a running
/// index into the block-kind children so that sibling scope-creating
/// statements resolve to the correct child scope.
/// Result of attempting to descend through a statement: either a resolved
/// child scope (Some) or the number of scope-children the statement
/// consumed (used to keep the outer sibling-index in sync).
enum DescendResult {
    Resolved(ScopeId),
    Advance(usize),
}

fn descend_if_else(
    table: &SymbolTable,
    block_children: &[ScopeId],
    child_idx: usize,
    ie: &ast::IfElse,
    offset: usize,
    contains_offset: bool,
) -> DescendResult {
    if !contains_offset {
        return DescendResult::Advance(if ie.else_body.is_some() { 2 } else { 1 });
    }
    let mut idx = child_idx;
    if ie.then_body.span.start <= offset && offset <= ie.then_body.span.end {
        if let Some(&child) = block_children.get(idx) {
            return DescendResult::Resolved(find_deepest_block_scope(
                table,
                child,
                &ie.then_body,
                offset,
            ));
        }
    }
    idx += 1;
    if let Some(else_body) = &ie.else_body {
        if else_body.span.start <= offset && offset <= else_body.span.end {
            if let Some(&child) = block_children.get(idx) {
                return DescendResult::Resolved(find_deepest_block_scope(
                    table, child, else_body, offset,
                ));
            }
        }
    }
    DescendResult::Advance(if ie.else_body.is_some() { 2 } else { 1 })
}

fn descend_stmt(
    table: &SymbolTable,
    block_children: &[ScopeId],
    child_idx: usize,
    stmt: &ast::Statement,
    offset: usize,
) -> DescendResult {
    let contains_offset = stmt.span.start <= offset && offset <= stmt.span.end;
    match &stmt.kind {
        ast::StatementKind::Block(inner) => {
            if contains_offset {
                if let Some(&child) = block_children.get(child_idx) {
                    return DescendResult::Resolved(find_deepest_block_scope(
                        table, child, inner, offset,
                    ));
                }
            }
            DescendResult::Advance(1)
        }
        ast::StatementKind::For(for_loop) => {
            if contains_offset && for_loop.body.span.start <= offset {
                if let Some(&child) = block_children.get(child_idx) {
                    return DescendResult::Resolved(find_deepest_block_scope(
                        table,
                        child,
                        &for_loop.body,
                        offset,
                    ));
                }
            }
            DescendResult::Advance(1)
        }
        ast::StatementKind::IfElse(ie) => descend_if_else(
            table,
            block_children,
            child_idx,
            ie,
            offset,
            contains_offset,
        ),
        ast::StatementKind::While(w) => {
            if contains_offset && w.body.span.start <= offset {
                if let Some(&child) = block_children.get(child_idx) {
                    return DescendResult::Resolved(find_deepest_block_scope(
                        table, child, &w.body, offset,
                    ));
                }
            }
            DescendResult::Advance(1)
        }
        _ => DescendResult::Advance(0),
    }
}

fn find_deepest_block_scope(
    table: &SymbolTable,
    body_scope: ScopeId,
    block: &ast::Block,
    offset: usize,
) -> ScopeId {
    let block_children: Vec<ScopeId> = table
        .scopes
        .get(body_scope)
        .children
        .iter()
        .filter(|&&c| table.scopes.get(c).kind == ScopeKind::Block)
        .copied()
        .collect();

    let mut child_idx = 0usize;
    for stmt in &block.stmts {
        match descend_stmt(table, &block_children, child_idx, stmt, offset) {
            DescendResult::Resolved(s) => return s,
            DescendResult::Advance(n) => child_idx += n,
        }
    }

    body_scope
}

/// Walk up the scope chain to find the enclosing Template or Function scope.
fn find_enclosing_body_scope(table: &SymbolTable, scope: ScopeId) -> ScopeId {
    let mut current = scope;
    loop {
        let s = table.scopes.get(current);
        match s.kind {
            ScopeKind::Template | ScopeKind::Function => return current,
            ScopeKind::File => return current,
            _ => match s.parent {
                Some(p) => current = p,
                None => return current,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;
    use crate::symbol_table::SymbolTable;

    fn build_table(source: &str) -> (crate::ast::File, SymbolTable) {
        let (ast, _) = parser::parse(source);
        let mut table = SymbolTable::new();
        table.index_file("test.circom", &ast);
        (ast, table)
    }

    // ── Top-level completion ──────────────────────────────────────

    #[test]
    fn top_level_keywords() {
        let src = "pragma circom \"2.2.3\";\n";
        let (ast, table) = build_table(src);
        let ctx = detect_context(src, src.len(), &ast, &table, "test.circom");
        assert_eq!(ctx, CompletionContext::TopLevel);

        let items = completions(&ctx, &table, "test.circom");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"template"));
        assert!(labels.contains(&"function"));
        assert!(labels.contains(&"bus"));
        assert!(labels.contains(&"include"));
        assert!(labels.contains(&"pragma"));
    }

    #[test]
    fn no_signal_completions_at_top_level() {
        let src = "pragma circom \"2.2.3\";\ntemplate Foo() { signal input x; }\n";
        let (ast, table) = build_table(src);
        let ctx = detect_context(src, src.len(), &ast, &table, "test.circom");
        assert_eq!(ctx, CompletionContext::TopLevel);

        let items = completions(&ctx, &table, "test.circom");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"Foo"));
        assert!(!labels.contains(&"x"));
    }

    // ── Template body completion ──────────────────────────────────

    #[test]
    fn template_body_keywords_and_signals() {
        let src = "template Foo(n) {\n    signal input x;\n    \n}";
        let (ast, table) = build_table(src);
        // Cursor at the blank line inside the template body.
        let offset = src.find("    \n}").unwrap() + 4;
        let ctx = detect_context(src, offset, &ast, &table, "test.circom");
        match &ctx {
            CompletionContext::TemplateBody(_) => {}
            other => panic!("expected TemplateBody, got {other:?}"),
        }

        let items = completions(&ctx, &table, "test.circom");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"signal input"));
        assert!(labels.contains(&"var"));
        assert!(labels.contains(&"component"));
        assert!(labels.contains(&"if"));
        assert!(labels.contains(&"for"));
        assert!(labels.contains(&"x"));
        assert!(labels.contains(&"n"));
    }

    #[test]
    fn template_name_completion_for_component() {
        let src = "template Adder(n) { signal input a; }\ntemplate Main() {\n    component c = \n}";
        let (ast, table) = build_table(src);
        let offset = src.find("component c = \n").unwrap() + 14;
        let ctx = detect_context(src, offset, &ast, &table, "test.circom");
        match &ctx {
            CompletionContext::TemplateBody(_) => {}
            other => panic!("expected TemplateBody, got {other:?}"),
        }

        let items = completions(&ctx, &table, "test.circom");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"Adder"));
    }

    // ── Dot completion: component.signal ───────────────────���──────

    #[test]
    fn dot_completion_component_signal() {
        let src = concat!(
            "template Inner() {\n",
            "    signal input a;\n",
            "    signal output b;\n",
            "}\n",
            "template Outer() {\n",
            "    component c = Inner();\n",
            "    c.\n",
            "}\n",
        );
        let (ast, table) = build_table(src);
        let offset = src.find("c.\n").unwrap() + 2;
        let ctx = detect_context(src, offset, &ast, &table, "test.circom");
        match &ctx {
            CompletionContext::DotAccess { receiver, .. } => {
                assert_eq!(receiver, "c");
            }
            other => panic!("expected DotAccess, got {other:?}"),
        }

        let items = completions(&ctx, &table, "test.circom");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"a"));
        assert!(labels.contains(&"b"));
    }

    // ── Dot completion: bus.field ─────���───────────────────────────

    #[test]
    fn dot_completion_bus_field() {
        let src = concat!(
            "pragma circom \"2.2.0\";\n",
            "bus Point() {\n",
            "    signal x;\n",
            "    signal y;\n",
            "}\n",
            "template Foo() {\n",
            "    signal input Point() p;\n",
            "    p.\n",
            "}\n",
        );
        let (ast, table) = build_table(src);
        let offset = src.find("p.\n").unwrap() + 2;
        let ctx = detect_context(src, offset, &ast, &table, "test.circom");
        match &ctx {
            CompletionContext::DotAccess { receiver, .. } => {
                assert_eq!(receiver, "p");
            }
            other => panic!("expected DotAccess, got {other:?}"),
        }

        let items = completions(&ctx, &table, "test.circom");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"x"));
        assert!(labels.contains(&"y"));
    }

    // ── Dot completion: signal.tag ───────���────────────────────────

    #[test]
    fn dot_completion_signal_tag() {
        let src = "template Foo() {\n    signal input {mytag, othertag} x;\n    x.\n}\n";
        let (ast, table) = build_table(src);
        let offset = src.find("x.\n").unwrap() + 2;
        let ctx = detect_context(src, offset, &ast, &table, "test.circom");
        match &ctx {
            CompletionContext::DotAccess { receiver, .. } => {
                assert_eq!(receiver, "x");
            }
            other => panic!("expected DotAccess, got {other:?}"),
        }

        let items = completions(&ctx, &table, "test.circom");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"mytag"));
        assert!(labels.contains(&"othertag"));
    }

    // ── Pragma completion ────────────────────────────────────────

    #[test]
    fn pragma_version_completion() {
        let src = "pragma circom ";
        let (ast, table) = build_table(src);
        let ctx = detect_context(src, src.len(), &ast, &table, "test.circom");
        assert_eq!(ctx, CompletionContext::Pragma);

        let items = completions(&ctx, &table, "test.circom");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"2.2.3"));
        assert!(labels.contains(&"2.0.0"));
        assert!(labels.contains(&"2.1.0"));
    }

    // ── Include completion ──────��────────────────────────────────

    #[test]
    fn include_path_detection() {
        let src = "include \"";
        let (ast, table) = build_table(src);
        let ctx = detect_context(src, src.len(), &ast, &table, "test.circom");
        assert_eq!(ctx, CompletionContext::Include);
    }

    // ── Context detection helpers ────────────────────────────────

    #[test]
    fn detect_dot_receiver_simple() {
        assert_eq!(detect_dot_receiver("comp."), Some("comp".to_string()));
        assert_eq!(detect_dot_receiver("my_bus."), Some("my_bus".to_string()));
        assert_eq!(detect_dot_receiver("x.ta"), Some("x".to_string()));
        assert_eq!(detect_dot_receiver("hello"), None);
        assert_eq!(detect_dot_receiver(""), None);
    }

    #[test]
    fn is_in_pragma_test() {
        assert!(is_in_pragma("pragma circom "));
        assert!(is_in_pragma("pragma circom \"2."));
        assert!(!is_in_pragma("pragma circom \"2.2.3\";"));
        assert!(!is_in_pragma("template Foo"));
        assert!(!is_in_pragma(""));
    }

    #[test]
    fn completion_items_have_correct_kinds() {
        let src = concat!(
            "template Foo() { signal input x; }\n",
            "function bar() { var y; return y; }\n",
        );
        let (_ast, table) = build_table(src);
        let ctx = CompletionContext::TopLevel;
        let items = completions(&ctx, &table, "test.circom");

        let foo_item = items.iter().find(|i| i.label == "Foo").unwrap();
        assert_eq!(foo_item.kind, Some(CompletionItemKind::CLASS));

        let bar_item = items.iter().find(|i| i.label == "bar").unwrap();
        assert_eq!(bar_item.kind, Some(CompletionItemKind::FUNCTION));
    }

    #[test]
    fn completion_items_include_snippets_for_templates() {
        let src = "template Adder(n) { signal input x; }\n";
        let (_ast, table) = build_table(src);
        let ctx = CompletionContext::TopLevel;
        let items = completions(&ctx, &table, "test.circom");

        let adder = items.iter().find(|i| i.label == "Adder").unwrap();
        assert_eq!(adder.insert_text, Some("Adder(${1})".to_string()));
        assert_eq!(adder.insert_text_format, Some(InsertTextFormat::SNIPPET));
    }
}
