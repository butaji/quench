//! Quench's runtime starts here and stays deliberately small.
//!
//! OXC owns syntax, scopes, and symbols. The runtime will consume queried
//! facts and execute only residual operations produced by the reducer.
//!
//! The frozen direction is:
//!
//! `source -> OXC -> ProgramDb facts -> partial evaluator -> residual Ops -> VM`
//!
//! The first implementation should establish the semantic kernel and the
//! compact representations (`Value`, `HeapRef`, `Shape`, `Frame`, `Code`,
//! `Fact`, and `Continuation`) before adding breadth.

mod arrays;
mod blocks;
mod branch;
mod builtins;
mod conditional;
mod construct;
mod control_flow;
mod exceptions;
pub mod execute;
pub mod facts;
mod functions;
mod globals;
mod identifiers;
mod literal;
mod locals;
mod logical;
mod loops;
mod methods;
mod objects;
pub mod ops;
mod properties;
pub mod reduce;
mod semantic;
mod sequences;
mod special;
mod statement_control;
mod statements;
mod switch;
mod templates;
mod transparent;
mod unary;
pub mod value;
