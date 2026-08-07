//! Quench's owned post-frontend representation.
//!
//! OXC allocations end with parsing. The parser lowers into this type before
//! the interpreter sees a program, so execution never borrows OXC AST nodes.
//! The current representation reuses the compact runtime `Program` layout;
//! this named boundary allows its storage to evolve independently.

pub use crate::ast::Program as QuenchIr;
