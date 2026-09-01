//! VM DSL: Native | Fast | Dynamic, Arena + GC.
//!
//! Layer and storage are independent. NIR | FIR | DIR filter one HIR enum.
//! Wasm enters as Native. Arena holds linear memory and unboxed locals; GC
//! holds structs/arrays/exns. QuickJS is the JS layer on top, not store GC.

pub mod build_profile;
mod bulk;
mod wasm_atomic;
pub mod dynamic;
pub mod fast;
pub mod hir;
pub mod hir_gc;
pub mod gc;
mod host_jobs;
pub mod instance;
pub mod interp;
pub mod wasm;
pub mod layer;
pub mod mir;
pub mod native;
pub mod slot;
pub mod unwind;
pub use host_jobs::install_host_job_pump;

mod arrays;
mod atomics;
pub use atomics::expire_async_waiters;
pub mod benchmark;
mod bigint;
mod binding_patterns;
mod blocks;
mod branch;
mod builtin_meta;
pub mod builtins;
pub mod capability;
mod classes;
mod collections;
pub mod completion;
mod conditional;
mod construct;
mod continuation;
mod control_flow;
mod conversion;
pub use conversion::is_callable;
pub use conversion::to_number;
pub use conversion::to_string;
pub mod date;
mod disposable_stack;
mod environment;
mod equality;
mod exceptions;
pub mod execute;
pub mod execution_trace;
pub mod facts;
mod finalization_registry;
mod function_code;
mod function_parameters;
mod functions;
mod functions_dynamic;
mod functions_write;
mod generator;
mod global_environment;
mod globals;
pub mod hardware_counters;
pub mod heap;
pub mod host_api;
mod identifiers;
pub mod identity;
mod intl;
mod json;
pub use json::parse as parse_json;
pub mod ir;
mod literal;
mod locals;
mod logical;
mod loops;
pub mod machine;
mod math;
mod methods;
pub mod module_bindings;
mod number_fmt;
mod objects;
pub mod operation;
pub mod ops;
mod ops_meta;
mod own_keys;
mod private_environment;
mod private_slots;
mod promise;
pub use promise::{
    drain_microtasks_all as drain_promise_jobs, has_pending_jobs as has_pending_promise_jobs,
    has_pending_unhandled_rejections, promise_then, reject_promise, resolve_promise,
    take_unhandled_rejections,
};
mod properties;
mod property_define;
pub mod protocol;
mod proxy;
pub mod reduce;
mod reduce_support;
mod reflect;
pub mod regexp;
mod regexp_backend;
mod regexp_emoji_data;
mod regexp_native;
pub mod register_file;
pub mod resource;
mod semantic;
mod semantic_catch;
mod semantic_early;
mod sequences;
mod special;
mod statement_control;
mod statements;
mod strings;
mod super_scope;
mod switch;
pub mod tagged_value;
mod templates;
mod temporal;
mod transparent;
mod typed_array_base64;
mod typed_array_ops;
mod typed_array_prototype;
mod unary;
mod using_early;
mod using_scope;
pub mod value;
pub mod vm;
mod with_scope;

/// Drop per-realm derived caches between independent fixture executions.
/// Resetters replace their containers so a pathological fixture's capacity is
/// not retained by the worker for the rest of a stage.
pub fn reset_fixture_caches() {
    construct::reset_weak_refs();
    global_environment::reset_global_bindings();
    loops::reset_fixture_state();
    module_bindings::reset_module_jobs();
    private_environment::reset_fixture_state();
    reflect::reset_fixture_state();
    intl::tolocale::symbol::reset_fixture_state();
    promise::reset_fixture_state();
    templates::reset_tagged_template_cache();
    value::reset_object_layout_cache();
    regexp::reset_compiled_cache();
}
