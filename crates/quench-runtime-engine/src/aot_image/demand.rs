//! What an image needs of the runtime that runs it: its demand (TOD-2228, step 1 of the size plan). It
//! is derived from the image alone, never stored beside it: every run-time address a body names is
//! a relocation (`Symbol`), and every value it folds is a recipe. A static executable's linker keeps
//! what the demand names; the rest of the runtime is unreachable from the image.
//!
//! Owner: image driver (`aot_image::lower`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{AotImage, HeapValueRecipe, Symbol, symbols::SymbolTable};
use quench_runtime::bytecode::{ResidualProgram, Traits};
use std::collections::BTreeSet;

/// The runtime an image reaches.
#[derive(Default, Debug)]
pub(crate) struct Demand {
    /// The lane helpers its bodies call, by name (`LaneHelper`).
    pub(crate) helpers: BTreeSet<&'static str>,
    /// The realm intrinsics its bodies fold (`Native::ALL`), by name.
    pub(crate) intrinsics: BTreeSet<String>,
    /// The global bindings it reads: the values its bodies fold, and every global name its records
    /// read by name (a generic record or the interpreter reads them at run time).
    pub(crate) globals: BTreeSet<Box<str>>,
    /// The records its base bodies run as the generic semantic row, which the interpreter's
    /// semantics serve (none means the image needs no interpreter).
    pub(crate) generic_records: usize,
    /// Whether it holds Wasm rows (the Wasm tier links them).
    pub(crate) wasm: bool,
}

/// The demand of `image`, the image of `program`.
pub(crate) fn of(image: &AotImage, program: &ResidualProgram) -> Demand {
    let mut demand = Demand { wasm: image.wasm.is_some(), ..Demand::default() };
    let bodies = image.functions.iter().flat_map(|function| std::iter::once(&function.base.code).chain(function.lane.as_ref().map(|lane| &lane.code)));
    let rows = image.wasm.iter().flat_map(|wasm| wasm.functions.iter().flatten());
    for relocation in bodies.chain(rows).flat_map(|code| code.relocations.iter()) {
        match SymbolTable::decode(image, relocation.namespace, relocation.key) {
            Some(Symbol::Helper(helper)) => {
                demand.helpers.extend(crate::compiled::LaneHelper::name(helper as usize));
            }
            Some(Symbol::HeapValue(key)) => match image.heap_values.get(key as usize) {
                Some(HeapValueRecipe::Intrinsic(id)) => {
                    demand.intrinsics.extend(quench_runtime::tier_surface::Native::ALL.get(*id as usize).map(|native| format!("{native:?}")));
                }
                Some(HeapValueRecipe::Global(atom)) => demand.globals.extend(image.atoms.get(*atom as usize).cloned()),
                _ => {}
            },
            _ => {}
        }
    }
    demand.generic_records = image.functions.iter().map(|function| function.base.generic.len()).sum();
    for index in 0..program.function_count() {
        let Some(function) = program.function_get(index) else { continue };
        for instruction in function.instructions() {
            if instruction.op().traits().has(Traits::GLOBAL_NAME_ACCESS) {
                demand.globals.extend(program.atoms.get(instruction.atom_index() as usize).map(Box::<str>::from));
            }
        }
    }
    demand
}

/// The records whose rows call the general step helpers (`Step`, `VmStep`): the interpreter's semantics,
/// by opcode, most first, base bodies by the record each call lies in and lane bodies counted apart.
pub(crate) fn step_records(image: &AotImage, program: &ResidualProgram) -> String {
    let stepping = |relocation: &quench_runtime::vm::jit_backend::relocatable_code::Relocation| matches!(SymbolTable::decode(image, relocation.namespace, relocation.key),
        Some(Symbol::Helper(helper)) if matches!(crate::compiled::LaneHelper::name(helper as usize), Some("Step" | "VmStep")));
    let mut by_op = std::collections::BTreeMap::<String, usize>::new();
    let mut lane = 0;
    for function in &image.functions {
        let code: Vec<_> = program.function(function.id as usize).instructions().collect();
        let mut starts: Vec<(u32, usize)> = function.base.pc_offsets.iter().enumerate().filter(|(_, offset)| **offset != super::base::NO_ENTRY).map(|(pc, offset)| (*offset, pc)).collect();
        starts.sort_unstable();
        for relocation in function.base.code.relocations.iter().filter(|relocation| stepping(relocation)) {
            let at = starts.partition_point(|(offset, _)| *offset as usize <= relocation.offset as usize);
            let name = at.checked_sub(1).and_then(|at| code.get(starts[at].1)).map_or("?", |instruction| quench_runtime::bytecode::Op::NAMES[instruction.op() as usize]);
            *by_op.entry(name.to_string()).or_default() += 1;
        }
        lane += function.lane.iter().flat_map(|lane| lane.code.relocations.iter()).filter(|relocation| stepping(relocation)).count();
    }
    let mut by_op: Vec<_> = by_op.into_iter().collect();
    by_op.sort_by(|a, b| b.1.cmp(&a.1));
    format!("lane {lane}, base {by_op:?}")
}

impl std::fmt::Display for Demand {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let list = |items: Vec<&str>| items.join(" ");
        write!(out, "{} helpers [{}], {} intrinsics [{}], {} globals [{}], {} generic records{}",
            self.helpers.len(), list(self.helpers.iter().copied().collect()),
            self.intrinsics.len(), list(self.intrinsics.iter().map(String::as_str).collect()),
            self.globals.len(), list(self.globals.iter().map(|name| &**name).collect()),
            self.generic_records, if self.wasm { ", Wasm rows" } else { "" })
    }
}
