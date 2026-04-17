//! Symbol types for the Circom symbol table.
//!
//! Each symbol represents a named entity in a Circom program: templates,
//! functions, buses, signals, variables, and components.

use crate::ast::SignalKind;
use crate::span::Span;

/// Unique identifier for a symbol within the symbol table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

/// Unique identifier for a scope within the scope tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);

/// The kind of scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeKind {
    /// File-level scope (templates, functions, buses).
    File,
    /// Template body scope.
    Template,
    /// Function body scope.
    Function,
    /// Bus definition scope.
    Bus,
    /// Block scope (for/if/while).
    Block,
}

/// The kind of a symbol.
#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    /// A template definition.
    Template(TemplateSymbol),
    /// A function definition.
    Function(FunctionSymbol),
    /// A bus definition (v2.2.0+).
    Bus(BusSymbol),
    /// A signal declaration.
    Signal(SignalSymbol),
    /// A variable declaration.
    Variable,
    /// A component declaration.
    Component(ComponentSymbol),
    /// A template/function parameter.
    Parameter,
}

/// Extra info for template symbols.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateSymbol {
    pub params: Vec<String>,
    pub is_custom: bool,
    pub is_parallel: bool,
    /// The scope containing the template's body.
    pub body_scope: ScopeId,
}

/// Extra info for function symbols.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionSymbol {
    pub params: Vec<String>,
    /// The scope containing the function's body.
    pub body_scope: ScopeId,
}

/// Extra info for bus symbols.
#[derive(Debug, Clone, PartialEq)]
pub struct BusSymbol {
    pub params: Vec<String>,
    /// The scope containing the bus's fields.
    pub body_scope: ScopeId,
}

/// Extra info for signal symbols.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalSymbol {
    pub kind: SignalKind,
    pub tags: Vec<String>,
    pub bus_type: Option<String>,
    pub dimensions: usize,
}

/// Extra info for component symbols.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentSymbol {
    pub template_name: Option<String>,
}

/// A symbol in the symbol table.
#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    /// The symbol's unique identifier.
    pub id: SymbolId,
    /// The symbol's name.
    pub name: String,
    /// The kind of symbol with type-specific information.
    pub kind: SymbolKind,
    /// Source location where the symbol is defined.
    pub span: Span,
    /// The scope this symbol belongs to.
    pub scope: ScopeId,
    /// The file path this symbol is defined in.
    pub file: String,
}

/// A diagnostic produced during symbol resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct SymbolDiagnostic {
    pub span: Span,
    pub message: String,
    pub kind: DiagnosticKind,
    /// The file this diagnostic belongs to.
    pub file: String,
}

/// The kind of symbol diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// A symbol was declared more than once in the same scope.
    DuplicateSymbol,
    /// A referenced symbol was not found.
    UndeclaredSymbol,
    /// Assigning to an input signal inside a template body.
    AssignToInput,
    /// Using `=` (variable assign) on a signal.
    VarAssignToSignal,
    /// Using signal assign (`<==`, `<--`) on a variable.
    SignalAssignToVar,
    /// Template parameter count mismatch on component instantiation.
    ParameterCountMismatch,
    /// Constraint does not fit quadratic form (A * B + C = 0).
    NonQuadraticConstraint,
    /// `<--` assignment without a corresponding `===` constraint (warning).
    UnsafeSignalAssignment,
    /// Using signals inside a function body (signals only valid in templates).
    SignalInFunction,
    /// A signal that is expected to carry a tag has lost it (e.g., assigning
    /// an untagged signal to a `{binary}` output).
    TagLoss,
    /// An assignment to a tagged input signal is missing a required tag.
    MissingRequiredTag,
    /// Unknown signal on a component — either not an input/output or no such
    /// template field exists.
    UnknownComponentSignal,
    /// An output signal of an instantiated component is never read (warning).
    UnusedComponentOutput,
    /// A template input was never driven by the caller (warning).
    MissingComponentInput,
    /// An output signal of the enclosing template is never assigned.
    UnderconstrainedOutput,
    /// Bus-typed signal assigned from a different bus type (`<==` / `<--`).
    BusTypeMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_id_equality() {
        assert_eq!(SymbolId(0), SymbolId(0));
        assert_ne!(SymbolId(0), SymbolId(1));
    }

    #[test]
    fn scope_id_equality() {
        assert_eq!(ScopeId(0), ScopeId(0));
        assert_ne!(ScopeId(0), ScopeId(1));
    }

    #[test]
    fn symbol_construction() {
        let sym = Symbol {
            id: SymbolId(0),
            name: "Adder".to_string(),
            kind: SymbolKind::Template(TemplateSymbol {
                params: vec!["n".to_string()],
                is_custom: false,
                is_parallel: false,
                body_scope: ScopeId(1),
            }),
            span: Span::new(0, 10),
            scope: ScopeId(0),
            file: "main.circom".to_string(),
        };
        assert_eq!(sym.name, "Adder");
        match &sym.kind {
            SymbolKind::Template(t) => {
                assert_eq!(t.params, vec!["n"]);
                assert!(!t.is_custom);
            }
            _ => panic!("expected template"),
        }
    }

    #[test]
    fn diagnostic_kinds() {
        let diag = SymbolDiagnostic {
            span: Span::new(5, 10),
            message: "duplicate template 'Foo'".to_string(),
            kind: DiagnosticKind::DuplicateSymbol,
            file: "main.circom".to_string(),
        };
        assert_eq!(diag.kind, DiagnosticKind::DuplicateSymbol);
    }
}
