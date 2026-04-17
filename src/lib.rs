// Enforce the repo-wide complexity gates: every function must be
// under 50 lines AND have cognitive complexity ≤ 20, matching the
// Go / TS / Python thresholds. Thresholds live in clippy.toml.
#![warn(clippy::too_many_lines)]
#![warn(clippy::cognitive_complexity)]

pub mod ast;
pub mod circomlib_docs;
pub mod constraint_checker;
pub mod formatter;
pub mod incremental;
pub mod lexer;
pub mod parser;
pub mod pretty_print;
pub mod scope;
pub mod server;
pub mod span;
pub mod symbol;
pub mod symbol_table;
pub mod type_checker;
pub mod underconstrained;
pub mod visitor;
pub mod walker;

pub use scope::ScopeTree;
pub use span::{LineCol, LineIndex};
pub use symbol::{Symbol, SymbolId};
pub use symbol_table::SymbolTable;
pub use visitor::Visitor;
pub use walker::Walker;
