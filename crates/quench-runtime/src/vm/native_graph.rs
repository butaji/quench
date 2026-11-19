//! Native control flow over statically selected shared semantic operations.

use super::native_object::{
    NATIVE_DATA_NAMESPACE, NATIVE_FUNCTION_NAMESPACE, NativeData, NativeFunction,
    NativeFunctionAddress,
};
use super::native_operands::NativeJsOperands;
use super::specialize::{CallThreaded, CallThreadedExtension, InstructionSite, Specializer};
use crate::staged::machine::{Binary, Lane, MachineAccess, MachineFlow, MachineScalar, Relation};
use crate::bytecode::{Function, Op, OPCODE_COUNT, ProgramKind, ResidualProgram, opcode_name};
use cranelift_codegen::{
    ir::{self, MemFlagsData},
    isa::TargetIsa,
};
use cranelift_frontend::FunctionBuilder;
use std::{num::NonZeroUsize, ops::Range};

#[path = "native_graph/object_parts.rs"]
mod object_parts;
#[path = "native_graph/dispatch.rs"]
mod dispatch;
pub(super) use dispatch::{lower_body, lower_region_dispatch, region_call_arguments};
use object_parts::{entry_table_data, import_operations, lower_lane_bodies, lower_object_entry, lower_object_regions};

use crate::vm::compiled::tier_policy::DEFAULT_REGION_INSTRUCTION_BUDGET;
/// The image's ABI header import is its first function symbol.
const ABI_SYMBOL_INDEX: u32 = 0;
/// Data records an image publishes between its functions' operands and the fallback tables: the narrow table, then the atom table.
const NARROW_TABLE_COUNT: usize = 1 + super::compiled::NATIVE_ATOM_TABLE_OFFSET as usize;

/// How executable memory receives the call-threaded graph. Both publications
/// residualize the same graph over the same semantic entries; they differ in
/// when the residual shape is compiled (`native_stencil`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeJsBackend {
    /// Copy and patch shapes compiled once per process. Falls back to
    /// `Cranelift` where the target has no stencil library.
    #[default]
    Stencil,
    /// Lower every function (or region) through Cranelift.
    Cranelift,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeJsCodegenOptions {
    /// Bounds the Cranelift working CFG; stencil publication has no CFG.
    pub region_instruction_budget: NonZeroUsize,
    pub backend: NativeJsBackend,
}

impl Default for NativeJsCodegenOptions {
    fn default() -> Self {
        Self {
            region_instruction_budget: DEFAULT_REGION_INSTRUCTION_BUDGET,
            backend: NativeJsBackend::default(),
        }
    }
}

impl NativeJsCodegenOptions {
    fn region_count(self, count: usize) -> usize {
        let width = self.region_instruction_budget.get();
        if count > width { count.div_ceil(width) } else { 0 }
    }

    pub(super) fn regions(self, count: usize) -> Option<Vec<Range<usize>>> {
        let width = self.region_instruction_budget.get();
        let regions = self.region_count(count);
        (regions != 0).then(|| {
            (0..regions)
                .map(|index| {
                    let start = index * width;
                    start..start.saturating_add(width).min(count)
                })
                .collect()
        })
    }
}

/// Link inputs for all functions in one source program, including its entry.
/// Semantic imports name compiler-owned operations, never process addresses.
pub struct NativeJsModule {
    pub functions: Vec<NativeFunction>,
    pub data: Vec<NativeData>,
    entry_table: usize,
}

impl NativeJsModule {
    /// Linker symbol for the ABI header followed by function-ID-ordered entries.
    pub fn entry_table_symbol(&self) -> &str {
        &self.data[self.entry_table].name
    }
}

/// Lower every function before execution. Deferred function bodies are resolved
/// through the program's existing authority, just as they are for the JIT.
pub fn lower_native_js_module(
    program: &ResidualProgram,
    namespace: &str,
) -> Result<NativeJsModule, String> {
    lower_native_js_module_with_options(program, namespace, NativeJsCodegenOptions::default())
}

pub fn lower_native_js_module_with_options(
    program: &ResidualProgram,
    namespace: &str,
    options: NativeJsCodegenOptions,
) -> Result<NativeJsModule, String> {
    lower_native_js_module_for(program, namespace, options, &super::jit_backend::NativeTarget::default())
}

/// Lowers `program` for `target`: the CPU baseline and triple its object is built for.
pub fn lower_native_js_module_for(
    program: &ResidualProgram,
    namespace: &str,
    options: NativeJsCodegenOptions,
    target: &super::jit_backend::NativeTarget,
) -> Result<NativeJsModule, String> {
    if program.kind == ProgramKind::Wasm {
        return Err("JavaScript native graph requires a JS or TS program".into());
    }
    let isa = super::jit_backend::relocatable_isa(target)?;
    CallThreaded::new(ObjectExtension { isa, namespace, options, program, functions: Vec::new() }).specialize(program)
}

/// The call-threaded generating extension for linked objects: each semantic
/// entry is an imported symbol, each operand record is object data, and the
/// linker binds both.
struct ObjectExtension<'n> {
    isa: std::sync::Arc<dyn TargetIsa>,
    namespace: &'n str,
    options: NativeJsCodegenOptions,
    /// Guarded shared lowering (scalar, boolean and fallback regions) reads the bytecode.
    program: &'n ResidualProgram,
    functions: Vec<Vec<InstructionSite>>,
}

impl CallThreadedExtension for ObjectExtension<'_> {
    type Residual = NativeJsModule;

    fn function(&mut self, _function_id: u32, sites: Vec<InstructionSite>) -> Result<(), String> {
        self.functions.push(sites);
        Ok(())
    }

    fn finish(self) -> Result<NativeJsModule, String> {
        let mut functions = vec![NativeFunction::Import {
            name: crate::NATIVE_JS_ABI_SYMBOL.into(),
            signature: ir::Signature::new(self.isa.default_call_conv()),
        }];
        let bodies_in_lane = self.bodies_in_lane(&mut functions)?;
        let operations = import_operations(&mut functions, &self.functions, &*self.isa)?;
        let symbols = self.symbols(functions.len(), &bodies_in_lane)?;
        let mut data = Vec::with_capacity(self.program.function_count());
        let mut pending = Definitions::default();
        for (id, lane_body) in bodies_in_lane.into_iter().enumerate() {
            let entry = self.lower_function(id, lane_body, &operations, &symbols, &mut pending)?;
            data.push(self.operand_data(id)?);
            functions.push(NativeFunction::Definition { name: format!("{}_function_{id}", self.namespace), function: entry });
        }
        self.assemble(functions, data, pending, symbols.first_definition)
    }
}

/// Where an image's definitions start: its functions' symbols, then their private regions, the
/// lane bodies and their fallbacks.
struct Symbols {
    first_definition: u32,
    body_base: usize,
    fallback_base: usize,
}

/// The definitions the functions' entries refer to, published after the entries themselves.
#[derive(Default)]
struct Definitions {
    regions: Vec<NativeFunction>,
    bodies: Vec<NativeFunction>,
    fallbacks: Vec<NativeFunction>,
    fallback_tables: Vec<NativeData>,
}

impl ObjectExtension<'_> {
    /// Every function's lane body, importing the lane helper table where any lowers.
    fn bodies_in_lane(&self, functions: &mut Vec<NativeFunction>) -> Result<Vec<Option<ir::Function>>, String> {
        let lane_helpers = u32::try_from(functions.len()).map_err(|_| "native helper symbol count exceeds u32")?;
        let bodies_in_lane = lower_lane_bodies(self.program, self.options, &*self.isa, lane_helpers);
        if bodies_in_lane.iter().any(Option::is_some) {
            functions.push(NativeFunction::Import {
                name: crate::NATIVE_JS_LANE_HELPERS_SYMBOL.into(),
                signature: super::compiled::native_lane_helpers_signature(&*self.isa),
            });
        }
        Ok(bodies_in_lane)
    }

    /// The symbol layout after `defined` imports: a function's lane body publishes under its own
    /// symbol, after the functions and the private regions; its fallback after the lane bodies.
    fn symbols(&self, defined: usize, bodies_in_lane: &[Option<ir::Function>]) -> Result<Symbols, String> {
        let first_definition = u32::try_from(defined).map_err(|_| "native symbol count exceeds u32")?;
        let region_count = (0..self.program.function_count())
            .try_fold(0usize, |count, id| count.checked_add(self.options.region_count(self.program.function(id).code.len())))
            .ok_or("native region count overflow")?;
        let body_base = (first_definition as usize)
            .checked_add(self.program.function_count())
            .and_then(|base| base.checked_add(region_count))
            .ok_or("native lane body symbol count overflow")?;
        let fallback_base = body_base.checked_add(bodies_in_lane.iter().flatten().count())
            .ok_or("native fallback symbol count overflow")?;
        u32::try_from(fallback_base).map_err(|_| "native lane body symbol count exceeds u32")?;
        Ok(Symbols { first_definition, body_base, fallback_base })
    }

    /// Function `id`'s entry: a dispatcher over private regions where it passes the region
    /// budget, else its trampoline over lane body and fallback, else its call-threaded body.
    fn lower_function(
        &self, id: usize, lane_body: Option<ir::Function>, operations: &[Option<u32>], symbols: &Symbols,
        pending: &mut Definitions,
    ) -> Result<ir::Function, String> {
        let function_id = u32::try_from(id).map_err(|_| "native function count exceeds u32")?;
        if let Some(regions) = self.options.regions(self.program.function(id).code.len()) {
            self.lower_regions(function_id, &regions, operations, symbols, pending)
        } else if let Some(lane_body) = lane_body {
            self.lower_lane(function_id, lane_body, operations, symbols, pending)
        } else {
            lower_object_body(&self.functions[id], function_id, &*self.isa, operations, None)
        }
    }

    fn lower_regions(
        &self, function_id: u32, regions: &[Range<usize>], operations: &[Option<u32>], symbols: &Symbols,
        pending: &mut Definitions,
    ) -> Result<ir::Function, String> {
        let id = function_id as usize;
        let region_base = (symbols.first_definition as usize)
            .checked_add(self.program.function_count())
            .and_then(|base| base.checked_add(pending.regions.len()))
            .ok_or("native symbol count overflow")?;
        let region_end = region_base.checked_add(regions.len()).ok_or("native symbol count overflow")?;
        u32::try_from(region_end).map_err(|_| "native symbol count exceeds u32")?;
        for (index, region) in regions.iter().cloned().enumerate() {
            pending.regions.push(NativeFunction::Definition {
                name: format!("{}_function_{id}_region_{index}", self.namespace),
                function: lower_object_body(&self.functions[id], function_id, &*self.isa, operations, Some(region))?,
            });
        }
        let length = self.program.function(id).code.len();
        lower_object_regions(function_id, length, self.options, &*self.isa, (region_base, regions.len()))
    }

    fn lower_lane(
        &self, function_id: u32, lane_body: ir::Function, operations: &[Option<u32>], symbols: &Symbols,
        pending: &mut Definitions,
    ) -> Result<ir::Function, String> {
        let id = function_id as usize;
        let (body, namespace) = (self.program.function(id), self.namespace);
        let entries = (
            u32::try_from(symbols.body_base + pending.bodies.len()).map_err(|_| "native lane body symbol exceeds u32")?,
            u32::try_from(symbols.fallback_base + pending.fallbacks.len()).map_err(|_| "native fallback symbol exceeds u32")?,
        );
        let table = self.program.function_count().checked_add(NARROW_TABLE_COUNT)
            .and_then(|index| index.checked_add(pending.fallback_tables.len()))
            .and_then(|index| u32::try_from(index).ok())
            .ok_or("native fallback table count exceeds u32")?;
        pending.fallback_tables.push(object_fallback_table(
            body, &format!("{namespace}_function_{id}_fallback_table"), &*self.isa, operations,
        )?);
        pending.fallbacks.push(NativeFunction::Definition {
            name: format!("{namespace}_function_{id}_fallback"),
            function: lower_object_fallback(body, function_id, table, &*self.isa)?,
        });
        pending.bodies.push(NativeFunction::Definition { name: format!("{namespace}_function_{id}_body"), function: lane_body });
        lower_object_entry(function_id, body.code.len(), &*self.isa, entries)
    }

    /// Function `id`'s operand records as object data.
    fn operand_data(&self, id: usize) -> Result<NativeData, String> {
        let sites = &self.functions[id];
        let byte_len = sites
            .len()
            .checked_mul(std::mem::size_of::<NativeJsOperands>())
            .ok_or("native operand data size overflow")?;
        let mut bytes = Vec::with_capacity(byte_len);
        for site in sites {
            site.operands.append_native_bytes(&mut bytes);
        }
        Ok(NativeData {
            name: format!("{}_operands_{id}", self.namespace),
            bytes,
            alignment: std::mem::align_of::<NativeJsOperands>() as u64,
            writable: false,
            function_addresses: Vec::new(),
        })
    }

    /// The module of `functions` and `data` (one operand record set per function) followed by the
    /// narrow table, the fallback tables, `pending`'s definitions and the entry table.
    fn assemble(
        &self, mut functions: Vec<NativeFunction>, mut data: Vec<NativeData>, pending: Definitions,
        first_definition: u32,
    ) -> Result<NativeJsModule, String> {
        // Lane bodies decode packed inline fields through the narrow table, so every image
        // publishes it (`NarrowTable::Linked`).
        data.push(NativeData {
            name: format!("{}_narrow_upper_words", self.namespace),
            bytes: crate::value::NarrowLayout::decode_words().iter().flat_map(|word| word.to_ne_bytes()).collect(),
            alignment: std::mem::align_of::<u64>() as u64,
            writable: false,
            function_addresses: Vec::new(),
        });
        data.push(NativeData {
            name: format!("{}_atoms", self.namespace),
            bytes: (0..self.program.atoms.len() as u32).flat_map(u32::to_ne_bytes).collect(),
            alignment: std::mem::align_of::<u32>() as u64,
            writable: false,
            function_addresses: Vec::new(),
        });
        data.extend(pending.fallback_tables);
        functions.extend(pending.regions);
        functions.extend(pending.bodies);
        functions.extend(pending.fallbacks);
        u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?;
        let entry_table = data.len();
        let entries = (ABI_SYMBOL_INDEX, first_definition);
        data.push(entry_table_data(&*self.isa, self.namespace, self.program.function_count(), entries)?);
        Ok(NativeJsModule { functions, data, entry_table })
    }
}

fn lower_object_body(
    sites: &[InstructionSite],
    function_id: u32,
    isa: &dyn TargetIsa,
    operations: &[Option<u32>],
    region: Option<Range<usize>>,
) -> Result<ir::Function, String> {
    let mut object = ObjectOperations::new(function_id, isa, operations);
    lower_body(
        sites,
        function_id,
        isa,
        region,
        |builder, site, context, result| object.emit(builder, site.op, site.pc, context, result),
    )
}

struct ObjectOperations<'a> {
    function_id: u32,
    isa: &'a dyn TargetIsa,
    operations: &'a [Option<u32>],
    entries: Vec<Option<ir::FuncRef>>,
    operand_base: Option<ir::GlobalValue>,
}

impl<'a> ObjectOperations<'a> {
    fn new(function_id: u32, isa: &'a dyn TargetIsa, operations: &'a [Option<u32>]) -> Self {
        Self {
            function_id,
            isa,
            operations,
            entries: vec![None; OPCODE_COUNT],
            operand_base: None,
        }
    }

    fn emit(
        &mut self, builder: &mut FunctionBuilder<'_>, op: Op, pc: usize,
        context: ir::Value, result: ir::Value,
    ) -> ir::Value {
        let entry = *self.entries[op as usize].get_or_insert_with(|| {
            let signature = builder.import_signature(
                super::compiled::native_js_operation_signature(self.isa),
            );
            let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
                NATIVE_FUNCTION_NAMESPACE,
                self.operations[op as usize].expect("collected operation"),
            ));
            builder.import_function(ir::ExtFuncData {
                name: ir::ExternalName::user(name), signature, colocated: false, patchable: false,
            })
        });
        let base = *self.operand_base.get_or_insert_with(|| {
            super::jit_backend::declare_symbol(builder.func, NATIVE_DATA_NAMESPACE, self.function_id)
        });
        let address = builder.symbol_address(base);
        // Both callers bound the instruction count, so this offset is host-addressable.
        let offset = i64::try_from(pc * std::mem::size_of::<NativeJsOperands>())
            .expect("host addressable operand offset");
        let address = builder.raw_binary_imm(Binary::Add, address, offset);
        let pc = builder.raw_const(Lane::I64, pc as i64);
        builder.call_function(entry, &[context, pc, address, result])[0]
    }
}

fn object_fallback_table(
    function: &Function, name: &str, isa: &dyn TargetIsa, operations: &[Option<u32>],
) -> Result<NativeData, String> {
    let pointer_bytes = usize::from(isa.pointer_bytes());
    let size = function.code.len().checked_mul(pointer_bytes).ok_or("native fallback table size overflow")?;
    let mut addresses = Vec::with_capacity(function.code.len());
    for (pc, packed) in function.code.iter().copied().enumerate() {
        let op = function.widened(packed).op();
        addresses.push(NativeFunctionAddress {
            offset: u32::try_from(pc * pointer_bytes).map_err(|_| "native fallback table offset exceeds u32")?,
            function: operations[op as usize].expect("collected operation"),
        });
    }
    Ok(NativeData {
        name: name.into(), bytes: vec![0; size], alignment: pointer_bytes as u64,
        writable: false, function_addresses: addresses,
    })
}

/// `function_id`'s definition under `signature`.
fn native_function(function_id: u32, signature: ir::Signature) -> ir::Function {
    ir::Function::with_name_signature(ir::UserFuncName::user(NATIVE_FUNCTION_NAMESPACE, function_id), signature)
}

/// Starts the entry block of a native function: its parameters.
fn enter_native_entry(builder: &mut FunctionBuilder<'_>) -> Vec<ir::Value> {
    let entry = builder.new_block();
    builder.append_block_params_for_function_params(entry);
    builder.enter_open(entry);
    builder.block_params(entry).to_vec()
}

/// One exact semantic operation for a guarded native path's fallback.
fn lower_object_fallback(
    function: &Function, function_id: u32, table: u32, isa: &dyn TargetIsa,
) -> Result<ir::Function, String> {
    let count = u32::try_from(function.code.len()).map_err(|_| "native instruction count exceeds u32")?;
    let mut native = native_function(function_id, super::compiled::native_js_operation_signature(isa));
    let mut context = crate::vm::jit_backend::builder::take_context();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let params = enter_native_entry(&mut builder);
    let pc = params[1];
    let valid_pc = builder.new_block();
    let invalid = builder.new_block();
    let valid = builder.raw_compare_imm(Relation::UnsignedLess, pc, i64::from(count));
    builder.branch_if(valid, (valid_pc, &[]), (invalid, &[]));
    builder.enter(valid_pc);
    let table_base = super::jit_backend::symbol_address(&mut builder, NATIVE_DATA_NAMESPACE, table);
    let table_offset = builder.raw_binary_imm(Binary::Mul, pc, i64::from(isa.pointer_bytes()));
    let entry_address = builder.raw_binary(Binary::Add, table_base, table_offset);
    let entry = builder.raw_load(Lane::I64, MemFlagsData::trusted(), entry_address, 0);
    let operands_base = super::jit_backend::symbol_address(&mut builder, NATIVE_DATA_NAMESPACE, function_id);
    let operands_offset = builder.raw_binary_imm(Binary::Mul, pc, std::mem::size_of::<NativeJsOperands>() as i64);
    let operands = builder.raw_binary(Binary::Add, operands_base, operands_offset);
    let signature = builder.import_signature(super::compiled::native_js_operation_signature(isa));
    let status = builder.call_signature(signature, entry, &[params[0], pc, operands, params[3]])[0];
    builder.leave(&[status]);
    builder.enter(invalid);
    let status = builder.raw_const(Lane::I64, i64::from(super::compiled::NATIVE_JS_STOPPED));
    builder.leave(&[status]);
    builder.seal_all();
    builder.finalize(isa.frontend_config());
    crate::vm::jit_backend::builder::keep_context(context);
    Ok(native)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The linked image folds a whole-program global with a read-site guard: the expected
    /// value is embedded and compared with the live global cell at each read.
    #[test]
    fn linked_image_folds_a_whole_program_global() {
        let program = crate::Engine::specialize(
            "var SCALE = 3; function scale(x) { let s = 0; for (let i = 0; i < 4; i++) { s = (s + x * SCALE) | 0; } return s; } globalThis.answer = scale(2); globalThis.SCALE = 5; globalThis.answer2 = scale(2);",
            "frozen-global-lower.js").unwrap();
        let module = lower_native_js_module(&program, "frozen_global").unwrap();
        let ir = module.functions.iter().filter_map(|function| match function {
            NativeFunction::Definition { function, .. } => Some(function.display().to_string()),
            NativeFunction::Import { .. } => None,
        }).collect::<Vec<_>>().join("\n");
        // 3 is a tagged Number (`0x7ffd_0000_0000_0003`); the read-site guard compares the
        // live global cell with exactly that word.
        assert!(ir.contains("0x7ffd_0000_0000_0003"),
            "the expected global value is embedded:\n{ir}");
        assert!(ir.contains("icmp eq"), "the read-site guard compares the live cell:\n{ir}");
    }

    /// The linked image folds a whole-program prototype value with a read-site guard: the
    /// expected value is embedded and compared with the live slot, so a changed prototype
    /// fails the guard and the shared body computes the guest answer.
    #[test]
    fn linked_image_folds_a_whole_program_field() {
        let program = crate::Engine::specialize(
            "function C(){} C.prototype.k = 7; function f(a){ let s = 0; for (let i = 0; i < 8; i++) { s = (s + a[i & 1].k) | 0; } return s; } f([new C(), new C()]);",
            "frozen-field-lower.js").unwrap();
        let module = lower_native_js_module(&program, "frozen_field").unwrap();
        let ir = module.functions.iter().filter_map(|function| match function {
            NativeFunction::Definition { function, .. } => Some(function.display().to_string()),
            NativeFunction::Import { .. } => None,
        }).collect::<Vec<_>>().join("\n");
        // 7 is a tagged Number (`0x7ffd_0000_0000_0007`); the read-site guard compares the
        // live slot with exactly that word.
        assert!(ir.contains("0x7ffd_0000_0000_0007"),
            "the expected field value is embedded:\n{ir}");
        assert!(ir.contains("icmp eq"), "the read-site guard compares the live slot:\n{ir}");
    }

    #[test]
    fn native_objects_include_entry_and_deferred_functions_in_js_and_ts() {
        for program in [
            crate::Engine::specialize(
                "function outer(x) { function inner(y) { return y + 1; } return inner(x); } outer(41);",
                "graph.js",
            ).unwrap(),
            crate::Engine::specialize_typescript(
                "function outer(x: number): number { function inner(y: number): number { return y + 1; } return inner(x); } outer(41);",
                "graph.cts",
            ).unwrap(),
        ] {
            let module = lower_native_js_module(&program, "graph").unwrap();
            let definitions: Vec<_> = module.functions.iter().filter_map(|function| match function {
                NativeFunction::Definition { name, .. } => Some(name.as_str()),
                NativeFunction::Import { .. } => None,
            }).collect();
            assert!(definitions.len() >= program.function_count());
            assert!(definitions.contains(&"graph_function_0"), "root must execute natively");
            assert!(definitions.iter().any(|name| name.ends_with("_fallback")), "eligible functions use shared guarded lowering");
            let object = super::super::native_object::emit_native_module("graph", module.functions, module.data)
                .expect("all function and operand relocations resolve");
            assert!(!object.is_empty());
        }
    }

}
