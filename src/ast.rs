//! AST node types for Circom v2.2.3.
//!
//! Every node carries a [`Span`] for source-location tracking.

use crate::span::Span;

// ── Top-level file ──────────────────────────────────────────────────

/// A complete `.circom` source file.
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub span: Span,
    pub items: Vec<Item>,
}

/// A top-level item in a circom file.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Pragma(Pragma),
    Include(Include),
    TemplateDef(TemplateDef),
    FunctionDef(FunctionDef),
    BusDef(BusDef),
    MainComponent(MainComponent),
}

// ── Pragma & Include ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Pragma {
    pub span: Span,
    pub kind: PragmaKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PragmaKind {
    /// `pragma circom "2.2.3";`  — stored as the version string
    Version(Version),
    /// `pragma custom_templates;`
    CustomTemplates,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Include {
    pub span: Span,
    pub path: String,
}

// ── Template ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct TemplateDef {
    pub span: Span,
    pub name: Identifier,
    pub params: Vec<Identifier>,
    pub body: Block,
    pub is_custom: bool,
    pub is_parallel: bool,
    pub is_extern: bool,
}

// ── Function ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub span: Span,
    pub name: Identifier,
    pub params: Vec<Identifier>,
    pub body: Block,
}

// ── Bus ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct BusDef {
    pub span: Span,
    pub name: Identifier,
    pub params: Vec<Identifier>,
    pub body: Vec<BusMember>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BusMember {
    Signal(SignalDecl),
    Bus(BusFieldDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BusFieldDecl {
    pub span: Span,
    pub bus_type: BusType,
    pub tags: Vec<Identifier>,
    pub name: Identifier,
    pub dimensions: Vec<Expression>,
}

// ── Main component ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct MainComponent {
    pub span: Span,
    pub public_signals: Vec<Identifier>,
    pub expr: Expression,
}

// ── Statements ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub span: Span,
    pub stmts: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub span: Span,
    pub kind: StatementKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StatementKind {
    VarDecl(VarDecl),
    SignalDecl(SignalDecl),
    ComponentDecl(ComponentDecl),
    BusDecl(BusInstanceDecl),

    /// `expr <op> expr` where op is =, <==, ==>, <--, -->, ===
    Assignment(AssignStmt),
    /// Compound assignment: +=, -=, etc.
    CompoundAssign(CompoundAssignStmt),
    /// Constraint equality: `expr === expr`
    ConstraintEq(ConstraintEqStmt),

    /// Tuple assignment: `(a, b, _) <== expr`
    TupleAssign(TupleAssignStmt),

    IfElse(IfElse),
    For(ForLoop),
    While(WhileLoop),
    Return(ReturnStmt),
    Log(LogStmt),
    Assert(AssertStmt),

    /// `expr++` or `expr--`
    Increment(Expression),
    Decrement(Expression),

    /// A bare expression statement (e.g., function call).
    Expression(Expression),

    /// Block statement `{ ... }`
    Block(Block),

    /// Error recovery placeholder.
    Error,
}

// ── Declarations ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct VarDecl {
    pub span: Span,
    pub names: Vec<VarDeclEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VarDeclEntry {
    pub name: Identifier,
    pub dimensions: Vec<Expression>,
    pub init: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignalDecl {
    pub span: Span,
    pub kind: SignalKind,
    pub tags: Vec<Identifier>,
    pub names: Vec<SignalDeclEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignalDeclEntry {
    pub name: Identifier,
    pub dimensions: Vec<Expression>,
    pub init: Option<(SignalAssignOp, Expression)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    Input,
    Output,
    Intermediate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalAssignOp {
    SafeLeft,   // <==
    UnsafeLeft, // <--
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDecl {
    pub span: Span,
    pub is_parallel: bool,
    pub names: Vec<ComponentDeclEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDeclEntry {
    pub name: Identifier,
    pub dimensions: Vec<Expression>,
    pub init: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BusInstanceDecl {
    pub span: Span,
    pub bus_type: BusType,
    pub signal_kind: SignalKind,
    pub tags: Vec<Identifier>,
    pub name: Identifier,
    pub dimensions: Vec<Expression>,
    pub init: Option<(SignalAssignOp, Expression)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BusType {
    pub span: Span,
    pub name: Identifier,
    pub args: Vec<Expression>,
}

// ── Assignment statements ───────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct AssignStmt {
    pub lhs: Expression,
    pub op: AssignOp,
    pub rhs: Expression,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Eq,          // =
    SafeLeft,    // <==
    SafeRight,   // ==>
    UnsafeLeft,  // <--
    UnsafeRight, // -->
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompoundAssignStmt {
    pub lhs: Expression,
    pub op: CompoundOp,
    pub rhs: Expression,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundOp {
    AddAssign,
    SubAssign,
    MulAssign,
    PowAssign,
    DivAssign,
    IntDivAssign,
    ModAssign,
    ShlAssign,
    ShrAssign,
    BitAndAssign,
    BitOrAssign,
    BitXorAssign,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintEqStmt {
    pub lhs: Expression,
    pub rhs: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TupleAssignStmt {
    /// Each element is either Some(expr) or None (underscore placeholder).
    pub targets: Vec<Option<Expression>>,
    pub op: AssignOp,
    pub rhs: Expression,
}

// ── Control flow ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct IfElse {
    pub cond: Expression,
    pub then_body: Block,
    pub else_body: Option<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForLoop {
    pub init: Box<Statement>,
    pub cond: Expression,
    pub step: Box<Statement>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WhileLoop {
    pub cond: Expression,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnStmt {
    pub value: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogStmt {
    pub args: Vec<LogArg>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogArg {
    Expr(Expression),
    String(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssertStmt {
    pub expr: Expression,
}

// ── Expressions ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    pub span: Span,
    pub kind: Box<ExpressionKind>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExpressionKind {
    /// Numeric literal
    Number(String),

    /// Identifier
    Ident(String),

    /// Unary operator
    Unary(UnaryOp, Expression),

    /// Binary operator
    Binary(Expression, BinaryOp, Expression),

    /// Ternary `cond ? then : else`
    Ternary(Expression, Expression, Expression),

    /// Array index: `expr[index]`
    Index(Expression, Expression),

    /// Member access: `expr.field`
    Member(Expression, Identifier),

    /// Function / template call: `name(args)`
    Call(Expression, Vec<Expression>),

    /// Anonymous component invocation: `Template(params)(inputs)`
    AnonymousComp(AnonymousComp),

    /// Array literal `[a, b, c]`
    ArrayLit(Vec<Expression>),

    /// Parenthesized expression
    Paren(Expression),

    /// `parallel expr`
    Parallel(Expression),

    /// Underscore placeholder `_`
    Underscore,

    /// Error recovery placeholder.
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnonymousComp {
    /// The template expression (name or call)
    pub template: Expression,
    /// Template arguments
    pub template_args: Vec<Expression>,
    /// Input signal assignments
    pub inputs: Vec<AnonCompInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnonCompInput {
    /// Positional input
    Positional(Expression),
    /// Named input: `name <== expr`
    Named(Identifier, Expression),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,    // -
    Not,    // !
    BitNot, // ~
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    IntDiv,
    Mod,
    Pow,
    Shl,
    Shr,
    BitAnd,
    BitOr,
    BitXor,
    And,
    Or,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

// ── Common types ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Identifier {
    pub span: Span,
    pub name: String,
}
