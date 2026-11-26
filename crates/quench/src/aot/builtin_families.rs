//! The intrinsic families an image's program reaches, derived from the compiled programs'
//! atoms, constants, functions and global reads. A family nothing names is not installed.
//!
//! Soundness: any dynamic source, `with`, reflective global aliasing or dynamic member call
//! keeps `BuiltinFamilies::ALL`, since such a program can name any intrinsic at run time.

use quench_runtime::{BuiltinFamilies, ResidualProgram};
use std::collections::BTreeSet;

/// The globals and method atoms that prove each family reachable.
pub(super) fn of<'a>(programs: impl Iterator<Item = &'a ResidualProgram>) -> BuiltinFamilies {
    let mut families = BuiltinFamilies::NONE;
    for program in programs {
        families = families.union(program_families(program));
    }
    families
}

fn program_families(program: &ResidualProgram) -> BuiltinFamilies {
    let atoms: BTreeSet<&str> = program.atoms.iter().collect();
    if dynamic(&atoms, program) {
        return BuiltinFamilies::ALL;
    }
    let mut families = BuiltinFamilies::NONE;
    for (names, family) in FAMILY_NAMES {
        if names.iter().any(|name| atoms.contains(name)) {
            families = families.union(*family);
        }
    }
    if !quench_runtime::vm::Vm::regexp_literal_matchers(program).is_empty() {
        families = families.union(BuiltinFamilies::REGEXP);
    }
    if (&program.constants).into_iter().any(|constant| matches!(constant, quench_runtime::bytecode::Constant::BigInt(_))) {
        families = families.union(BuiltinFamilies::BIGINT);
    }
    if (0..program.function_count()).any(|index| program.function_get(index).is_some_and(|function| function.is_async)) {
        families = families.union(BuiltinFamilies::PROMISE_ASYNC);
    }
    families
}

/// Whether the program can reach an intrinsic its atoms do not name: dynamic source, `with`,
/// a global object alias or reflection over computed names.
fn dynamic(atoms: &BTreeSet<&str>, program: &ResidualProgram) -> bool {
    program.kind == quench_runtime::bytecode::ProgramKind::Eval
        || DYNAMIC_NAMES.iter().any(|name| atoms.contains(name))
        || (0..program.function_count()).any(|index| program.function_get(index).is_some_and(|function| function.has_dynamic_scope_access()))
}

const DYNAMIC_NAMES: &[&str] = &[
    "eval", "Function", "globalThis", "window", "self", "Reflect", "Proxy", "with",
];

const FAMILY_NAMES: &[(&[&str], BuiltinFamilies)] = &[
    (&["RegExp", "match", "matchAll", "replace", "replaceAll", "search", "split"], BuiltinFamilies::REGEXP),
    (&[
        "ArrayBuffer", "SharedArrayBuffer", "DataView", "Atomics", "structuredClone",
        "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array",
        "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array",
    ], BuiltinFamilies::TYPED_MEMORY),
    (&[
        "Map", "Set", "WeakMap", "WeakSet", "WeakRef", "FinalizationRegistry", "groupBy", "group",
    ], BuiltinFamilies::COLLECTIONS),
    (&["Date"], BuiltinFamilies::DATE),
    (&["BigInt"], BuiltinFamilies::BIGINT),
    (&["JSON"], BuiltinFamilies::JSON),
    (&["Promise", "AsyncGenerator", "AsyncIterator", "queueMicrotask"], BuiltinFamilies::PROMISE_ASYNC),
    (&["ShadowRealm"], BuiltinFamilies::SHADOW_REALM),
];
