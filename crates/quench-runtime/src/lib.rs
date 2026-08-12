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
mod binding_patterns;
mod blocks;
mod branch;
mod builtin_meta;
pub mod builtins;
mod classes;
mod collections;
mod completion;
mod conditional;
mod construct;
mod continuation;
mod control_flow;
mod conversion;
pub mod date;
mod environment;
mod equality;
mod exceptions;
pub mod execute;
pub mod facts;
mod function_parameters;
mod functions;
mod functions_dynamic;
mod functions_write;
mod generator;
mod global_environment;
mod globals;
mod identifiers;
mod intl;
mod json;
mod literal;
mod locals;
mod logical;
mod loops;
pub mod machine;
mod math;
mod methods;
mod number_fmt;
mod objects;
pub mod ops;
mod ops_meta;
mod own_keys;
mod private_environment;
mod private_slots;
mod promise;
mod properties;
mod property_define;
mod proxy;
pub mod reduce;
mod reduce_support;
mod reflect;
pub mod regexp;
mod semantic;
mod semantic_early;
mod sequences;
mod special;
mod statement_control;
mod statements;
mod strings;
mod super_scope;
mod switch;
mod templates;
mod transparent;
mod typed_array_ops;
mod typed_array_prototype;
mod unary;
pub mod value;
pub mod vm;
mod with_scope;
