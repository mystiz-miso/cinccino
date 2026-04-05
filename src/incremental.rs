//! Incremental parser for Circom source files.
//!
//! Caches top-level items from a previous parse and re-parses only the
//! affected region when the source text changes.  Unaffected items are
//! kept from the cache with their byte-spans shifted by the edit delta.

use crate::ast::{File, Item};
use crate::parser::ParseError;
use crate::span::Span;

// ── Edit description ───────────────────────────────────────────────

/// Describes a contiguous text edit in byte offsets.
#[derive(Debug, Clone)]
pub struct TextEdit {
    /// Byte offset where the edit starts (in the *old* source).
    pub start: usize,
    /// Number of bytes removed from the old text.
    pub removed: usize,
    /// Number of bytes inserted at `start` in the new text.
    pub inserted: usize,
}

impl TextEdit {
    /// Signed byte-length change (`inserted - removed`).
    fn delta(&self) -> isize {
        self.inserted as isize - self.removed as isize
    }

    /// Byte range that was replaced in the old source.
    fn old_range(&self) -> std::ops::Range<usize> {
        self.start..self.start + self.removed
    }
}

// ── Cached item ────────────────────────────────────────────────────

/// A top-level item together with its byte range in the source.
#[derive(Debug, Clone)]
struct CachedItem {
    item: Item,
    /// Inclusive start byte offset.
    start: usize,
    /// Exclusive end byte offset.
    end: usize,
    /// Parse errors originating from this item's region.
    errors: Vec<ParseError>,
}

// ── IncrementalParser ──────────────────────────────────────────────

/// An incremental parser that caches top-level items and re-parses
/// only the region affected by an edit.
#[derive(Debug, Clone)]
pub struct IncrementalParser {
    entries: Vec<CachedItem>,
}

impl IncrementalParser {
    /// Perform an initial full parse.
    pub fn parse(source: &str) -> Self {
        let (file, errors) = crate::parser::parse(source);
        let entries: Vec<CachedItem> = file
            .items
            .into_iter()
            .map(|item| {
                let span = item_span(&item);
                CachedItem {
                    item,
                    start: span.start,
                    end: span.end,
                    errors: Vec::new(),
                }
            })
            .collect();
        let mut parser = Self { entries };
        parser.distribute_errors(errors);
        parser
    }

    /// Apply an incremental text edit and re-parse only the affected
    /// region.  Returns the updated AST and parse errors.
    pub fn update(&mut self, new_source: &str, edit: &TextEdit) -> (File, Vec<ParseError>) {
        let delta = edit.delta();
        let old_range = edit.old_range();

        // Find indices of items that overlap the edited range.
        let first_affected = self
            .entries
            .iter()
            .position(|e| e.end > old_range.start && e.start < old_range.end);
        let last_affected = self
            .entries
            .iter()
            .rposition(|e| e.end > old_range.start && e.start < old_range.end);

        // Determine re-parse window in the *new* source.
        let (reparse_start, reparse_end, replace_range) =
            if let (Some(first), Some(last)) = (first_affected, last_affected) {
                let start = self.entries[first].start;
                // End of the last affected item in new-source coordinates.
                let raw_end = self.entries[last]
                    .end
                    .checked_add_signed(delta)
                    .expect("span underflow in reparse window");
                // The re-parse window must cover at least the inserted text.
                let end = raw_end
                    .max(edit.start + edit.inserted)
                    .min(new_source.len());
                (start, end.max(start), first..last + 1)
            } else {
                // Edit is in whitespace between items (or at start/end
                // of file outside any item).  Re-parse the gap.
                let start = edit.start;
                let end = (edit.start + edit.inserted).min(new_source.len());
                // Insert point: the index where new items would go.
                let insert_at = self
                    .entries
                    .iter()
                    .position(|e| e.start >= edit.start)
                    .unwrap_or(self.entries.len());
                (start, end, insert_at..insert_at)
            };

        // Re-parse the affected region as if it were a standalone file.
        let region = &new_source[reparse_start..reparse_end];
        let (partial_file, partial_errors) = crate::parser::parse(region);

        // Offset all items from the partial parse so their spans are
        // relative to the full source.
        let new_items: Vec<CachedItem> = partial_file
            .items
            .into_iter()
            .map(|item| {
                let span = item_span(&item);
                CachedItem {
                    item: offset_item(item, reparse_start),
                    start: span.start + reparse_start,
                    end: span.end + reparse_start,
                    errors: Vec::new(),
                }
            })
            .collect();

        // Offset errors from partial parse.
        let new_errors: Vec<ParseError> = partial_errors
            .into_iter()
            .map(|mut e| {
                e.span.start += reparse_start;
                e.span.end += reparse_start;
                e
            })
            .collect();

        // Shift spans and errors of items after the affected region.
        for entry in self.entries[replace_range.end..].iter_mut() {
            entry.start = entry
                .start
                .checked_add_signed(delta)
                .expect("span underflow in entry start");
            entry.end = entry
                .end
                .checked_add_signed(delta)
                .expect("span underflow in entry end");
            shift_item_spans(&mut entry.item, delta);
            for err in &mut entry.errors {
                err.span.start = err
                    .span
                    .start
                    .checked_add_signed(delta)
                    .expect("span underflow in error start");
                err.span.end = err
                    .span
                    .end
                    .checked_add_signed(delta)
                    .expect("span underflow in error end");
            }
        }

        // Splice in the newly parsed items.
        self.entries.splice(replace_range.clone(), new_items);

        // Distribute new errors among the newly spliced items.
        self.distribute_errors_in_range(new_errors, replace_range.start);

        self.to_file_and_errors(new_source)
    }

    /// Build the current `File` and error list from cached state.
    pub fn to_file_and_errors(&self, source: &str) -> (File, Vec<ParseError>) {
        let items: Vec<Item> = self.entries.iter().map(|e| e.item.clone()).collect();
        let span = if items.is_empty() {
            Span::new(0, source.len())
        } else {
            Span::new(
                self.entries.first().unwrap().start,
                self.entries.last().unwrap().end,
            )
        };
        let file = File { span, items };
        let errors: Vec<ParseError> = self
            .entries
            .iter()
            .flat_map(|e| e.errors.iter().cloned())
            .collect();
        (file, errors)
    }

    /// How many cached top-level items are stored.
    pub fn item_count(&self) -> usize {
        self.entries.len()
    }

    /// Distribute errors into cached entries by matching error spans to
    /// item byte ranges.  Errors that don't fall inside any item are
    /// assigned to the nearest following entry (or the last entry).
    fn distribute_errors(&mut self, errors: Vec<ParseError>) {
        for err in errors {
            let idx = self
                .entries
                .iter()
                .position(|e| err.span.start >= e.start && err.span.start < e.end)
                .or_else(|| self.entries.iter().position(|e| e.start >= err.span.start))
                .unwrap_or_else(|| self.entries.len().saturating_sub(1));
            if !self.entries.is_empty() {
                self.entries[idx].errors.push(err);
            }
        }
    }

    /// Distribute errors among entries starting at `range_start`,
    /// covering all newly spliced items.
    fn distribute_errors_in_range(&mut self, errors: Vec<ParseError>, range_start: usize) {
        for err in errors {
            let idx = self.entries[range_start..]
                .iter()
                .position(|e| err.span.start >= e.start && err.span.start < e.end)
                .map(|i| i + range_start)
                .or_else(|| {
                    self.entries[range_start..]
                        .iter()
                        .position(|e| e.start >= err.span.start)
                        .map(|i| i + range_start)
                })
                .unwrap_or_else(|| if range_start > 0 { range_start - 1 } else { 0 });
            if !self.entries.is_empty() {
                self.entries[idx].errors.push(err);
            }
        }
    }
}

// ── Span helpers ───────────────────────────────────────────────────

fn item_span(item: &Item) -> Span {
    match item {
        Item::Pragma(p) => p.span,
        Item::Include(i) => i.span,
        Item::TemplateDef(t) => t.span,
        Item::FunctionDef(f) => f.span,
        Item::BusDef(b) => b.span,
        Item::MainComponent(m) => m.span,
    }
}

/// Shift every span inside an `Item` by `offset` bytes (positive).
fn offset_item(mut item: Item, offset: usize) -> Item {
    shift_item_spans(&mut item, offset as isize);
    item
}

fn shift_item_spans(item: &mut Item, delta: isize) {
    match item {
        Item::Pragma(p) => shift_span(&mut p.span, delta),
        Item::Include(i) => shift_span(&mut i.span, delta),
        Item::TemplateDef(t) => {
            shift_span(&mut t.span, delta);
            shift_ident(&mut t.name, delta);
            for p in &mut t.params {
                shift_ident(p, delta);
            }
            shift_block(&mut t.body, delta);
        }
        Item::FunctionDef(f) => {
            shift_span(&mut f.span, delta);
            shift_ident(&mut f.name, delta);
            for p in &mut f.params {
                shift_ident(p, delta);
            }
            shift_block(&mut f.body, delta);
        }
        Item::BusDef(b) => {
            shift_span(&mut b.span, delta);
            shift_ident(&mut b.name, delta);
            for p in &mut b.params {
                shift_ident(p, delta);
            }
            for member in &mut b.body {
                shift_bus_member(member, delta);
            }
        }
        Item::MainComponent(m) => {
            shift_span(&mut m.span, delta);
            for sig in &mut m.public_signals {
                shift_ident(sig, delta);
            }
            shift_expr(&mut m.expr, delta);
        }
    }
}

fn shift_span(span: &mut Span, delta: isize) {
    if *span == Span::dummy() {
        return;
    }
    span.start = span
        .start
        .checked_add_signed(delta)
        .expect("span underflow");
    span.end = span.end.checked_add_signed(delta).expect("span underflow");
}

fn shift_ident(id: &mut crate::ast::Identifier, delta: isize) {
    shift_span(&mut id.span, delta);
}

fn shift_block(block: &mut crate::ast::Block, delta: isize) {
    shift_span(&mut block.span, delta);
    for stmt in &mut block.stmts {
        shift_stmt(stmt, delta);
    }
}

fn shift_stmt(stmt: &mut crate::ast::Statement, delta: isize) {
    use crate::ast::StatementKind::*;
    shift_span(&mut stmt.span, delta);
    match &mut stmt.kind {
        VarDecl(v) => {
            shift_span(&mut v.span, delta);
            for entry in &mut v.names {
                shift_ident(&mut entry.name, delta);
                for d in &mut entry.dimensions {
                    shift_expr(d, delta);
                }
                if let Some(init) = &mut entry.init {
                    shift_expr(init, delta);
                }
            }
        }
        SignalDecl(s) => {
            shift_span(&mut s.span, delta);
            for tag in &mut s.tags {
                shift_ident(tag, delta);
            }
            for entry in &mut s.names {
                shift_ident(&mut entry.name, delta);
                for d in &mut entry.dimensions {
                    shift_expr(d, delta);
                }
                if let Some((_, init)) = &mut entry.init {
                    shift_expr(init, delta);
                }
            }
        }
        ComponentDecl(c) => {
            shift_span(&mut c.span, delta);
            for entry in &mut c.names {
                shift_ident(&mut entry.name, delta);
                for d in &mut entry.dimensions {
                    shift_expr(d, delta);
                }
                if let Some(init) = &mut entry.init {
                    shift_expr(init, delta);
                }
            }
        }
        BusDecl(b) => {
            shift_span(&mut b.span, delta);
            shift_bus_type(&mut b.bus_type, delta);
            for tag in &mut b.tags {
                shift_ident(tag, delta);
            }
            shift_ident(&mut b.name, delta);
            for d in &mut b.dimensions {
                shift_expr(d, delta);
            }
            if let Some((_, init)) = &mut b.init {
                shift_expr(init, delta);
            }
        }
        Assignment(a) => {
            shift_expr(&mut a.lhs, delta);
            shift_expr(&mut a.rhs, delta);
        }
        CompoundAssign(c) => {
            shift_expr(&mut c.lhs, delta);
            shift_expr(&mut c.rhs, delta);
        }
        ConstraintEq(c) => {
            shift_expr(&mut c.lhs, delta);
            shift_expr(&mut c.rhs, delta);
        }
        TupleAssign(t) => {
            for e in t.targets.iter_mut().flatten() {
                shift_expr(e, delta);
            }
            shift_expr(&mut t.rhs, delta);
        }
        IfElse(ie) => {
            shift_expr(&mut ie.cond, delta);
            shift_block(&mut ie.then_body, delta);
            if let Some(else_body) = &mut ie.else_body {
                shift_block(else_body, delta);
            }
        }
        For(f) => {
            shift_stmt(f.init.as_mut(), delta);
            shift_expr(&mut f.cond, delta);
            shift_stmt(f.step.as_mut(), delta);
            shift_block(&mut f.body, delta);
        }
        While(w) => {
            shift_expr(&mut w.cond, delta);
            shift_block(&mut w.body, delta);
        }
        Return(r) => shift_expr(&mut r.value, delta),
        Log(l) => {
            for arg in &mut l.args {
                if let crate::ast::LogArg::Expr(e) = arg {
                    shift_expr(e, delta);
                }
            }
        }
        Assert(a) => shift_expr(&mut a.expr, delta),
        Increment(e) | Decrement(e) | Expression(e) => shift_expr(e, delta),
        Block(b) => shift_block(b, delta),
        Error => {}
    }
}

fn shift_expr(expr: &mut crate::ast::Expression, delta: isize) {
    use crate::ast::ExpressionKind::*;
    shift_span(&mut expr.span, delta);
    match expr.kind.as_mut() {
        Number(_) | Ident(_) | Underscore | Error => {}
        Unary(_, e) | Paren(e) | Parallel(e) => shift_expr(e, delta),
        Binary(l, _, r) => {
            shift_expr(l, delta);
            shift_expr(r, delta);
        }
        Ternary(c, t, e) => {
            shift_expr(c, delta);
            shift_expr(t, delta);
            shift_expr(e, delta);
        }
        Index(base, idx) => {
            shift_expr(base, delta);
            shift_expr(idx, delta);
        }
        Member(base, field) => {
            shift_expr(base, delta);
            shift_ident(field, delta);
        }
        Call(callee, args) => {
            shift_expr(callee, delta);
            for a in args {
                shift_expr(a, delta);
            }
        }
        AnonymousComp(ac) => {
            shift_expr(&mut ac.template, delta);
            for a in &mut ac.template_args {
                shift_expr(a, delta);
            }
            for input in &mut ac.inputs {
                match input {
                    crate::ast::AnonCompInput::Positional(e) => shift_expr(e, delta),
                    crate::ast::AnonCompInput::Named(id, e) => {
                        shift_ident(id, delta);
                        shift_expr(e, delta);
                    }
                }
            }
        }
        ArrayLit(elems) => {
            for e in elems {
                shift_expr(e, delta);
            }
        }
    }
}

fn shift_bus_member(member: &mut crate::ast::BusMember, delta: isize) {
    match member {
        crate::ast::BusMember::Signal(s) => {
            shift_span(&mut s.span, delta);
            for tag in &mut s.tags {
                shift_ident(tag, delta);
            }
            for entry in &mut s.names {
                shift_ident(&mut entry.name, delta);
                for d in &mut entry.dimensions {
                    shift_expr(d, delta);
                }
                if let Some((_, init)) = &mut entry.init {
                    shift_expr(init, delta);
                }
            }
        }
        crate::ast::BusMember::Bus(b) => {
            shift_span(&mut b.span, delta);
            shift_bus_type(&mut b.bus_type, delta);
            for tag in &mut b.tags {
                shift_ident(tag, delta);
            }
            shift_ident(&mut b.name, delta);
            for d in &mut b.dimensions {
                shift_expr(d, delta);
            }
        }
    }
}

fn shift_bus_type(bt: &mut crate::ast::BusType, delta: isize) {
    shift_span(&mut bt.span, delta);
    shift_ident(&mut bt.name, delta);
    for a in &mut bt.args {
        shift_expr(a, delta);
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse with full parser, return items for comparison.
    fn full_parse(source: &str) -> (File, Vec<ParseError>) {
        crate::parser::parse(source)
    }

    #[test]
    fn initial_parse_matches_full_parser() {
        let src = r#"pragma circom "2.2.3";
template Foo(n) { signal input a; }
function bar() { return 1; }
"#;
        let inc = IncrementalParser::parse(src);
        let (inc_file, inc_errors) = inc.to_file_and_errors(src);
        let (full_file, full_errors) = full_parse(src);
        assert_eq!(inc_file, full_file);
        assert_eq!(inc_errors, full_errors);
        assert_eq!(inc.item_count(), 3);
    }

    #[test]
    fn edit_inside_single_item_body() {
        let old_src = r#"pragma circom "2.2.3";
template Foo(n) { signal input a; }
function bar() { return 1; }
"#;
        let new_src = r#"pragma circom "2.2.3";
template Foo(n) { signal input a; signal output b; }
function bar() { return 1; }
"#;
        let mut inc = IncrementalParser::parse(old_src);
        // The edit inserts " signal output b;" inside the template body.
        // Old template body ends at the `}` before `\nfunction`.
        // Find the edit position: after "signal input a;" insert " signal output b;"
        let edit_pos = old_src.find("signal input a; }").unwrap() + "signal input a; ".len();
        let edit = TextEdit {
            start: edit_pos,
            removed: 0,
            inserted: "signal output b; ".len(),
        };

        let (inc_file, _) = inc.update(new_src, &edit);
        let (full_file, _) = full_parse(new_src);
        assert_eq!(inc_file, full_file);
    }

    #[test]
    fn edit_replaces_text_inside_function() {
        let old_src = "function foo() { return 1; }\nfunction bar() { return 2; }\n";
        let new_src = "function foo() { return 42; }\nfunction bar() { return 2; }\n";

        let mut inc = IncrementalParser::parse(old_src);
        // Replace "1" with "42" inside foo's return.
        let edit_start = old_src.find("return 1").unwrap() + "return ".len();
        let edit = TextEdit {
            start: edit_start,
            removed: 1,  // "1"
            inserted: 2, // "42"
        };

        let (inc_file, _) = inc.update(new_src, &edit);
        let (full_file, _) = full_parse(new_src);
        assert_eq!(inc_file, full_file);
    }

    #[test]
    fn edit_in_whitespace_between_items() {
        let old_src = "pragma circom \"2.2.3\";\n\nfunction foo() { return 1; }\n";
        // Insert a new template between pragma and function.
        let new_src = "pragma circom \"2.2.3\";\ntemplate T() {}\nfunction foo() { return 1; }\n";

        let mut inc = IncrementalParser::parse(old_src);
        let edit_start = old_src.find("\n\n").unwrap() + 1; // after first newline
        let edit = TextEdit {
            start: edit_start,
            removed: 1, // the second newline
            inserted: "template T() {}\n".len(),
        };

        let (inc_file, _) = inc.update(new_src, &edit);
        let (full_file, _) = full_parse(new_src);
        assert_eq!(inc_file, full_file);
    }

    #[test]
    fn spans_shift_correctly_after_edit() {
        let old_src = "function a() { return 1; }\nfunction b() { return 2; }\n";
        // Insert characters into function a, making it longer.
        let new_src = "function aaa() { return 1; }\nfunction b() { return 2; }\n";

        let mut inc = IncrementalParser::parse(old_src);
        let edit_start = old_src.find(" a(").unwrap() + 1;
        let edit = TextEdit {
            start: edit_start,
            removed: 1,  // "a"
            inserted: 3, // "aaa"
        };

        let (inc_file, _) = inc.update(new_src, &edit);
        let (full_file, _) = full_parse(new_src);
        assert_eq!(inc_file, full_file);
    }

    #[test]
    fn delete_an_entire_item() {
        let old_src = "pragma circom \"2.2.3\";\ntemplate T() {}\nfunction f() { return 0; }\n";
        let new_src = "pragma circom \"2.2.3\";\nfunction f() { return 0; }\n";

        let mut inc = IncrementalParser::parse(old_src);
        let template_start = old_src.find("template").unwrap();
        let template_end = old_src.find("template T() {}").unwrap() + "template T() {}\n".len();
        let edit = TextEdit {
            start: template_start,
            removed: template_end - template_start,
            inserted: 0,
        };

        let (inc_file, _) = inc.update(new_src, &edit);
        let (full_file, _) = full_parse(new_src);
        assert_eq!(inc_file, full_file);
    }

    #[test]
    fn empty_source() {
        let inc = IncrementalParser::parse("");
        assert_eq!(inc.item_count(), 0);
        let (file, errors) = inc.to_file_and_errors("");
        assert!(file.items.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn multiple_edits_sequential() {
        let src1 = "function a() { return 1; }\n";
        let src2 = "function a() { return 2; }\n";
        let src3 = "function a() { return 2; }\nfunction b() { return 3; }\n";

        let mut inc = IncrementalParser::parse(src1);

        // Edit 1: change "1" to "2"
        let pos = src1.find("1;").unwrap();
        inc.update(
            src2,
            &TextEdit {
                start: pos,
                removed: 1,
                inserted: 1,
            },
        );

        // Edit 2: append function b
        let pos2 = src2.len();
        let (file, _) = inc.update(
            src3,
            &TextEdit {
                start: pos2,
                removed: 0,
                inserted: "function b() { return 3; }\n".len(),
            },
        );

        let (full_file, _) = full_parse(src3);
        assert_eq!(file, full_file);
    }

    #[test]
    fn shift_template_with_various_statements() {
        // Template with many statement types after the edited item
        // exercises shift_item_spans(TemplateDef), shift_stmt for
        // VarDecl, SignalDecl, ComponentDecl, Assignment, ConstraintEq,
        // IfElse, For, While, Log, Assert, Return, and their child
        // expression variants.
        let old_src = concat!(
            "function f() { return 1; }\n",
            "template T(n) {\n",
            "    signal input a;\n",
            "    signal output b;\n",
            "    var x = 0;\n",
            "    component h = Poseidon(2);\n",
            "    x = a + b;\n",
            "    b <== a * 2;\n",
            "    if (n > 0) { x = 1; } else { x = 2; }\n",
            "    for (var i = 0; i < n; i++) { x += 1; }\n",
            "    while (x > 0) { x -= 1; }\n",
            "    log(x);\n",
            "    assert(x == 0);\n",
            "}\n",
        );
        let new_src = concat!(
            "function ff() { return 1; }\n",
            "template T(n) {\n",
            "    signal input a;\n",
            "    signal output b;\n",
            "    var x = 0;\n",
            "    component h = Poseidon(2);\n",
            "    x = a + b;\n",
            "    b <== a * 2;\n",
            "    if (n > 0) { x = 1; } else { x = 2; }\n",
            "    for (var i = 0; i < n; i++) { x += 1; }\n",
            "    while (x > 0) { x -= 1; }\n",
            "    log(x);\n",
            "    assert(x == 0);\n",
            "}\n",
        );

        let mut inc = IncrementalParser::parse(old_src);
        let edit_start = old_src.find(" f(").unwrap() + 1;
        let edit = TextEdit {
            start: edit_start,
            removed: 1,  // "f"
            inserted: 2, // "ff"
        };

        let (inc_file, inc_errors) = inc.update(new_src, &edit);
        let (full_file, full_errors) = full_parse(new_src);
        assert_eq!(inc_file, full_file);
        assert_eq!(inc_errors, full_errors);
    }

    #[test]
    fn shift_bus_and_main_component() {
        // Bus definition and main component after the edited item
        // exercises shift_item_spans for BusDef and MainComponent,
        // plus shift_bus_member and shift_bus_type.
        let old_src = concat!(
            "pragma circom 2.2.0;\n",
            "function f() { return 1; }\n",
            "bus Point() {\n",
            "    signal input x;\n",
            "    signal input y;\n",
            "}\n",
            "template Foo() { signal input a; }\n",
            "component main { public [a] } = Foo();\n",
        );
        let new_src = concat!(
            "pragma circom 2.2.0;\n",
            "function ff() { return 1; }\n",
            "bus Point() {\n",
            "    signal input x;\n",
            "    signal input y;\n",
            "}\n",
            "template Foo() { signal input a; }\n",
            "component main { public [a] } = Foo();\n",
        );

        let mut inc = IncrementalParser::parse(old_src);
        let edit_start = old_src.find(" f(").unwrap() + 1;
        let edit = TextEdit {
            start: edit_start,
            removed: 1,  // "f"
            inserted: 2, // "ff"
        };

        let (inc_file, inc_errors) = inc.update(new_src, &edit);
        let (full_file, full_errors) = full_parse(new_src);
        assert_eq!(inc_file, full_file);
        assert_eq!(inc_errors, full_errors);
    }

    #[test]
    fn shift_expressions_complex() {
        // Exercises shift_expr for Ternary, Index, Member, Call,
        // ArrayLit, Unary, Paren, and Binary variants.
        let old_src = concat!(
            "function f() { return 1; }\n",
            "function g(n) {\n",
            "    var a;\n",
            "    var b;\n",
            "    a = n > 0 ? 1 : 0;\n",
            "    b = (a + 1) * -n;\n",
            "    return a;\n",
            "}\n",
        );
        let new_src = concat!(
            "function ff() { return 1; }\n",
            "function g(n) {\n",
            "    var a;\n",
            "    var b;\n",
            "    a = n > 0 ? 1 : 0;\n",
            "    b = (a + 1) * -n;\n",
            "    return a;\n",
            "}\n",
        );

        let mut inc = IncrementalParser::parse(old_src);
        let edit_start = old_src.find(" f(").unwrap() + 1;
        let edit = TextEdit {
            start: edit_start,
            removed: 1,
            inserted: 2,
        };

        let (inc_file, inc_errors) = inc.update(new_src, &edit);
        let (full_file, full_errors) = full_parse(new_src);
        assert_eq!(inc_file, full_file);
        assert_eq!(inc_errors, full_errors);
    }

    #[test]
    fn shift_tuple_assign_and_block_stmt() {
        // Exercises shift_stmt for TupleAssign, CompoundAssign,
        // Increment, Decrement, Block, and Expression statements.
        let old_src = concat!(
            "function f() { return 1; }\n",
            "function g() {\n",
            "    var a;\n",
            "    var b;\n",
            "    (a, b) = (1, 2);\n",
            "    a++;\n",
            "    b--;\n",
            "    a += 1;\n",
            "    { a = 0; }\n",
            "    return a;\n",
            "}\n",
        );
        let new_src = concat!(
            "function ff() { return 1; }\n",
            "function g() {\n",
            "    var a;\n",
            "    var b;\n",
            "    (a, b) = (1, 2);\n",
            "    a++;\n",
            "    b--;\n",
            "    a += 1;\n",
            "    { a = 0; }\n",
            "    return a;\n",
            "}\n",
        );

        let mut inc = IncrementalParser::parse(old_src);
        let edit_start = old_src.find(" f(").unwrap() + 1;
        let edit = TextEdit {
            start: edit_start,
            removed: 1,
            inserted: 2,
        };

        let (inc_file, inc_errors) = inc.update(new_src, &edit);
        let (full_file, full_errors) = full_parse(new_src);
        assert_eq!(inc_file, full_file);
        assert_eq!(inc_errors, full_errors);
    }

    #[test]
    fn errors_preserved_across_items_after_edit() {
        // Template A has a syntax error; template B is valid.
        // Editing B should preserve A's error.
        let old_src = "template A() { signal input }\ntemplate B() { signal input b; }\n";
        let inc = IncrementalParser::parse(old_src);
        let (_, initial_errors) = inc.to_file_and_errors(old_src);
        assert!(
            !initial_errors.is_empty(),
            "template A should have a parse error"
        );

        // Edit template B: rename "b" to "bb" (no effect on A's error).
        let new_src = "template A() { signal input }\ntemplate B() { signal input bb; }\n";
        let mut inc = IncrementalParser::parse(old_src);
        let edit_start = old_src.find("input b;").unwrap() + "input ".len();
        let edit = TextEdit {
            start: edit_start,
            removed: 1,  // "b"
            inserted: 2, // "bb"
        };

        let (_, errors_after_edit) = inc.update(new_src, &edit);
        let (_, full_errors) = full_parse(new_src);
        assert_eq!(
            errors_after_edit, full_errors,
            "errors from unaffected items must be preserved after an edit to a different item"
        );
    }

    #[test]
    fn span_shift_all_statement_types() {
        // First item is a pragma, second template has many statement types.
        // Editing the pragma causes span shifts in all statements of the template.
        let old_src = concat!(
            "pragma circom 2.0.0;\n",
            "template T() {\n",
            "    var x = 1;\n",
            "    signal input a;\n",
            "    signal output b;\n",
            "    component c = Other();\n",
            "    b <== a;\n",
            "    x += 2;\n",
            "    a === b;\n",
            "    if (x) {\n",
            "        x = 2;\n",
            "    } else {\n",
            "        x = 3;\n",
            "    }\n",
            "    for (var i = 0; i < 10; i++) {\n",
            "        x = i;\n",
            "    }\n",
            "    while (x > 0) {\n",
            "        x--;\n",
            "    }\n",
            "    log(x);\n",
            "    assert(x == 0);\n",
            "    x++;\n",
            "    x;\n",
            "}\n",
        );
        // Change pragma version: "2.0.0" -> "2.10.0" (insert 1 char)
        let new_src = old_src.replacen("2.0.0", "2.10.0", 1);

        let mut inc = IncrementalParser::parse(old_src);
        let edit_start = old_src.find("2.0.0").unwrap() + 2; // after "2."
        let edit = TextEdit {
            start: edit_start,
            removed: 1,  // "0"
            inserted: 2, // "10"
        };

        let (inc_file, inc_errors) = inc.update(&new_src, &edit);
        let (full_file, full_errors) = full_parse(&new_src);
        assert_eq!(inc_file, full_file);
        assert_eq!(inc_errors, full_errors);
    }

    #[test]
    fn span_shift_expressions() {
        let old_src = concat!(
            "pragma circom 2.0.0;\n",
            "template T() {\n",
            "    var x = 1 + 2;\n",
            "    var y = x > 0 ? x : 0;\n",
            "    var z = [1, 2, 3];\n",
            "    var w = z[0];\n",
            "    var v = -x;\n",
            "    var u = (x);\n",
            "}\n",
        );
        let new_src = old_src.replacen("2.0.0", "2.10.0", 1);

        let mut inc = IncrementalParser::parse(old_src);
        let edit_start = old_src.find("2.0.0").unwrap() + 2;
        let edit = TextEdit {
            start: edit_start,
            removed: 1,
            inserted: 2,
        };

        let (inc_file, _) = inc.update(&new_src, &edit);
        let (full_file, _) = full_parse(&new_src);
        assert_eq!(inc_file, full_file);
    }

    #[test]
    fn span_shift_member_and_call() {
        let old_src = concat!(
            "pragma circom 2.0.0;\n",
            "template T() {\n",
            "    component c = Other();\n",
            "    var x = c.out;\n",
            "    var y = c.out + 1;\n",
            "}\n",
        );
        let new_src = old_src.replacen("2.0.0", "2.10.0", 1);

        let mut inc = IncrementalParser::parse(old_src);
        let edit_start = old_src.find("2.0.0").unwrap() + 2;
        let edit = TextEdit {
            start: edit_start,
            removed: 1,
            inserted: 2,
        };

        let (inc_file, _) = inc.update(&new_src, &edit);
        let (full_file, _) = full_parse(&new_src);
        assert_eq!(inc_file, full_file);
    }

    #[test]
    fn span_shift_for_step_variants() {
        let old_src = concat!(
            "pragma circom 2.0.0;\n",
            "template T() {\n",
            "    for (var i = 10; i > 0; i--) {\n",
            "        var x = i;\n",
            "    }\n",
            "    for (var j = 0; j < 10; j += 2) {\n",
            "        var y = j;\n",
            "    }\n",
            "}\n",
        );
        let new_src = old_src.replacen("2.0.0", "2.10.0", 1);

        let mut inc = IncrementalParser::parse(old_src);
        let edit_start = old_src.find("2.0.0").unwrap() + 2;
        let edit = TextEdit {
            start: edit_start,
            removed: 1,
            inserted: 2,
        };

        let (inc_file, _) = inc.update(&new_src, &edit);
        let (full_file, _) = full_parse(&new_src);
        assert_eq!(inc_file, full_file);
    }

    #[test]
    fn span_shift_bus_and_anon_comp() {
        let old_src = concat!(
            "pragma circom 2.0.0;\n",
            "bus MyBus() {\n",
            "    signal input a;\n",
            "}\n",
            "template T() {\n",
            "    signal input x;\n",
            "    signal output y;\n",
            "    y <== AnotherT()(x);\n",
            "}\n",
        );
        let new_src = old_src.replacen("2.0.0", "2.10.0", 1);

        let mut inc = IncrementalParser::parse(old_src);
        let edit_start = old_src.find("2.0.0").unwrap() + 2;
        let edit = TextEdit {
            start: edit_start,
            removed: 1,
            inserted: 2,
        };

        let (inc_file, _) = inc.update(&new_src, &edit);
        let (full_file, _) = full_parse(&new_src);
        assert_eq!(inc_file, full_file);
    }

    #[test]
    fn item_span_covers_all_variants() {
        // Test item_span for Include and MainComponent
        let src = concat!(
            "pragma circom 2.0.0;\n",
            "include \"lib.circom\";\n",
            "template T() { signal input a; }\n",
            "function f() { return 1; }\n",
            "component main = T();\n",
        );
        let inc = IncrementalParser::parse(src);
        assert_eq!(inc.item_count(), 5);
        let (file, errors) = inc.to_file_and_errors(src);
        let (full_file, full_errors) = full_parse(src);
        assert_eq!(file, full_file);
        assert_eq!(errors, full_errors);
    }
}
