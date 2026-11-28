//! What an image needs of the runtime that runs it: its demand (TOD-2228, step 1 of the size plan, C5 B1).
//! It is the closed-world contract between the lowering and the linker. It is derived from the image
//! alone, never stored beside it: every run-time address a body names is a relocation (`Symbol`), every
//! value it folds is a recipe, and every lane leave is an exit record. A static executable's linker keeps
//! what the demand names; the rest of the runtime is unreachable from the image.
//!
//! Owner: image driver (`aot_image::lower`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{AotImage, Symbol, symbols::{HeapValueRecipe, SymbolTable}};
use crate::compiled::LaneHelper;
use quench_regexp::backtrack::native::AheadKey;
use quench_runtime::bytecode::{ResidualProgram, Traits};
use quench_runtime::tier_surface::Native;
use quench_runtime::vm::engine_types::LaneImage;
use quench_runtime::vm::jit_backend::relocatable_code::RelocatableCode;
use std::collections::BTreeSet;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The runtime an image reaches.
#[derive(Default, Debug)]
pub struct Demand {
    /// The lane helpers its bodies call, by name (`LaneHelper`).
    pub helpers: BTreeSet<&'static str>,
    /// The realm natives its bodies fold (`HeapValueRecipe::Intrinsic`): the seed of the closed world's closure.
    pub natives: BTreeSet<Native>,
    /// The global bindings it reads: the values its bodies fold, and every global name its records
    /// read by name (a generic record or the interpreter reads them at run time).
    pub globals: BTreeSet<Box<str>>,
    /// The RegExp literal searches it carries (`AotImage::regexps`), by key.
    pub regexps: BTreeSet<AheadKey>,
    /// The records its base bodies run as the generic semantic row, which the interpreter's
    /// semantics serve (`needs_interpreter`).
    pub generic_records: usize,
    /// Whether it holds Wasm rows (the Wasm tier links them).
    pub wasm: bool,
    /// Whether a lane body names a symbol only a VM resolves (atoms, shapes, heap values, cells), so the
    /// image is linked in a VM at run time (`live::link_lane`) rather than by the system linker.
    pub needs_live_link: bool,
    /// Whether a lane body has leaves: an exit record, or a leave's continuation in its base body.
    pub needs_exits: bool,
    /// Whether the image reads its functions' source text (`Function.prototype.toString`). Set by the
    /// closed-world reachability pass (C2); the lowering alone never proves it.
    pub needs_source_text: bool,
    /// Whether the image reads Error stack positions. Set by the closed-world reachability pass (C2).
    pub needs_stack_positions: bool,
}

/// Which entries of the execution table an image's closed world needs (B2): an entry it does not need is
/// `None` in the linked image, and reaching it at run time is a typed abort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionTableSpec {
    /// The interpreter runs a generic record.
    pub interpret: bool,
    /// The tier policy takes over a frame that leaves its closed body.
    pub tier_up: bool,
    /// Source text is compiled at run time (`eval`, `Function`). Kept on until the source proof (E1) lands.
    pub compile_source: bool,
    /// The VM links a lane body from the image file.
    pub link_live: bool,
    /// The Wasm tier links the image's rows.
    pub wasm: bool,
}

impl Demand {
    /// Whether the image needs the interpreter: some base body runs a generic record.
    pub fn needs_interpreter(&self) -> bool {
        self.generic_records > 0
    }

    /// The natives the closed world must keep: the seed of the reachability closure (C2).
    pub fn native_roots(&self) -> &BTreeSet<Native> {
        &self.natives
    }

    /// The execution table entries the image needs. `compile_source` stays on: no proof here rules out
    /// a dynamic source, and a `None` reached at run time is an abort.
    pub fn execution_table(&self) -> ExecutionTableSpec {
        ExecutionTableSpec {
            interpret: self.needs_interpreter(),
            tier_up: self.needs_exits,
            compile_source: true,
            link_live: self.needs_live_link,
            wasm: self.wasm,
        }
    }

    /// Adds `other`'s demand to this one: the aggregate over the images of one compilation.
    pub fn absorb(&mut self, other: &Demand) {
        self.helpers.extend(other.helpers.iter().copied());
        self.natives.extend(other.natives.iter().copied());
        self.globals.extend(other.globals.iter().cloned());
        self.regexps.extend(other.regexps.iter().cloned());
        self.generic_records += other.generic_records;
        self.wasm |= other.wasm;
        self.needs_live_link |= other.needs_live_link;
        self.needs_exits |= other.needs_exits;
        self.needs_source_text |= other.needs_source_text;
        self.needs_stack_positions |= other.needs_stack_positions;
    }
}

/// The symbols of `code`'s relocations, `None` for a key no table names.
fn symbols<'a>(image: &'a AotImage, code: &'a RelocatableCode) -> impl Iterator<Item = Option<Symbol>> + 'a {
    code.relocations.iter().map(move |relocation| SymbolTable::decode(image, relocation.namespace, relocation.key))
}

/// The demand of `image`, the image of `program`.
pub(crate) fn of(image: &AotImage, program: &ResidualProgram) -> Demand {
    let mut demand = Demand { wasm: image.wasm.is_some(), ..Demand::default() };
    let base = image.functions.iter().map(|function| &function.base.code);
    let lanes = image.functions.iter().filter_map(|function| function.lane.as_ref());
    let rows = image.wasm.iter().flat_map(|wasm| wasm.functions.iter().flatten());
    for code in base.chain(lanes.clone().map(|lane| &lane.code)).chain(rows) {
        for symbol in symbols(image, code).flatten() {
            named(image, symbol, &mut demand);
        }
    }
    for lane in lanes {
        lane_demand(image, lane, &mut demand);
    }
    demand.generic_records = image.functions.iter().map(|function| function.base.generic.len()).sum();
    demand.regexps = image.regexps.iter().map(|regexp| regexp.key.clone()).collect();
    global_names(program, &mut demand);
    demand
}

/// What one symbol a body names needs of the runtime: its helper, or the native or global a recipe folds.
fn named(image: &AotImage, symbol: Symbol, demand: &mut Demand) {
    match symbol {
        Symbol::Helper(helper) => demand.helpers.extend(LaneHelper::name(helper as usize)),
        Symbol::HeapValue(key) => match image.heap_values.get(key as usize) {
            Some(HeapValueRecipe::Intrinsic(id)) => demand.natives.extend(Native::ALL.get(*id as usize).copied()),
            Some(HeapValueRecipe::Global(atom)) => demand.globals.extend(image.atoms.get(*atom as usize).cloned()),
            _ => {}
        },
        _ => {}
    }
}

/// What a lane body needs beyond its base body. The symbols the object linker places
/// (`object::import_symbols`) need no VM; every other symbol a lane names is resolved by a VM that loads
/// the image. A leave resumes its base body (`BasePc`) and records an exit site.
fn lane_demand(image: &AotImage, lane: &LaneImage, demand: &mut Demand) {
    demand.needs_exits |= !lane.exits.sites.is_empty();
    for symbol in symbols(image, &lane.code) {
        match symbol {
            Some(Symbol::Helper(_) | Symbol::Attention | Symbol::NarrowUpperWords | Symbol::CallRecord { .. } | Symbol::EntryStub) => {}
            Some(Symbol::BasePc { .. }) => demand.needs_exits = true,
            _ => demand.needs_live_link = true,
        }
    }
}

/// The global bindings the program's global name accesses read, by name.
fn global_names(program: &ResidualProgram, demand: &mut Demand) {
    for index in 0..program.function_count() {
        let Some(function) = program.function_get(index) else { continue };
        for instruction in function.instructions() {
            if instruction.op().traits().has(Traits::GLOBAL_NAME_ACCESS) {
                demand.globals.extend(program.atoms.get(instruction.atom_index() as usize).map(Box::<str>::from));
            }
        }
    }
}

impl std::fmt::Display for Demand {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let list = |items: Vec<String>| items.join(" ");
        write!(out, "{} helpers [{}], {} natives [{}], {} globals [{}], {} regexps, {} generic records{}{}{}{}{}",
            self.helpers.len(), list(self.helpers.iter().map(|helper| helper.to_string()).collect()),
            self.natives.len(), list(self.natives.iter().map(|native| format!("{native:?}")).collect()),
            self.globals.len(), list(self.globals.iter().map(|name| name.to_string()).collect()),
            self.regexps.len(), self.generic_records,
            if self.wasm { ", Wasm rows" } else { "" },
            if self.needs_live_link { ", live link" } else { "" },
            if self.needs_exits { ", exits" } else { "" },
            if self.needs_source_text { ", source text" } else { "" },
            if self.needs_stack_positions { ", stack positions" } else { "" })
    }
}
