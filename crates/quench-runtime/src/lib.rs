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
mod bigint;
mod blocks;
mod branch;
mod builtin_meta;
mod builtins;
mod collections;
mod conditional;
mod construct;
mod control_flow;
pub mod date;
mod exceptions;
pub mod execute;
pub mod facts;
mod functions;
mod globals;
mod identifiers;
mod intl;
mod literal;
mod locals;
mod logical;
mod loops;
mod math;
mod methods;
mod modules;
mod number_fmt;
mod objects;
pub mod ops;
mod packing;
mod promise;
mod properties;
mod proxy;
pub mod reduce;
mod reduce_support;
mod reflect;
pub mod regexp;
mod semantic;
mod sequences;
mod special;
mod statement_control;
mod statements;
mod strings;
mod switch;
mod templates;
mod transparent;
mod unary;
pub mod value;
pub mod vm;
