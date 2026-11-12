//! Native control flow over statically selected shared semantic operations.

use super::native_object::{
    NATIVE_DATA_NAMESPACE, NATIVE_FUNCTION_NAMESPACE, NativeData, NativeFunction,
    NativeFunctionAddress,
};
use super::native_operands::NativeJsOperands;
use super::specialize::{CallThreaded, CallThreadedExtension, Continuation, InstructionSite, Specializer};
use crate::bytecode::{Function, Op, OPCODE_COUNT, ProgramKind, ResidualProgram, opcode_name};
use cranelift_codegen::{
    ir::{self, InstBuilder, MemFlagsData, condcodes::IntCC, types},
    isa::TargetIsa,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use std::{num::NonZeroUsize, ops::Range};

#[path = "native_graph/object_parts.rs"]
mod object_parts;
use object_parts::{entry_table_data, import_operations, lower_lane_bodies, lower_object_entry, lower_object_regions};

// Bounds the working CFG, not the source function. Large unpartitioned native
// graphs exhausted 14 GiB on a 50 KiB input; callers may choose any nonzero budget.
const DEFAULT_REGION_INSTRUCTION_BUDGET: NonZeroUsize = NonZeroUsize::new(1024).unwrap();
const NATIVE_JS_REGION_EXIT: u32 = super::compiled::NATIVE_JS_INVALID_PC + 1;
// The shared entry ABI's memory-base word is unused by JavaScript bodies.
const REGION_START_PC_PARAMETER: usize = 1;
/// The image's ABI header import is its first function symbol.
const ABI_SYMBOL_INDEX: u32 = 0;
/// Data records an image publishes between its functions' operands and the fallback tables: the narrow table.
const NARROW_TABLE_COUNT: usize = 1;

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
    if program.kind == ProgramKind::Wasm {
        return Err("JavaScript native graph requires a JS or TS program".into());
    }
    let isa = super::jit_backend::target_isa_for(
        "speed",
        super::jit_backend::CodePublication::RelocatableObject,
    )
    .ok_or("native target unavailable")?;
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
        let lane_bodies = self.lane_bodies(&mut functions)?;
        let operations = import_operations(&mut functions, &self.functions, &*self.isa)?;
        let symbols = self.symbols(functions.len(), &lane_bodies)?;
        let mut data = Vec::with_capacity(self.program.function_count());
        let mut pending = Definitions::default();
        for (id, lane_body) in lane_bodies.into_iter().enumerate() {
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
    fn lane_bodies(&self, functions: &mut Vec<NativeFunction>) -> Result<Vec<Option<ir::Function>>, String> {
        let lane_helpers = u32::try_from(functions.len()).map_err(|_| "native helper symbol count exceeds u32")?;
        let lane_bodies = lower_lane_bodies(self.program, self.options, &*self.isa, lane_helpers);
        if lane_bodies.iter().any(Option::is_some) {
            functions.push(NativeFunction::Import {
                name: crate::NATIVE_JS_LANE_HELPERS_SYMBOL.into(),
                signature: super::compiled::native_lane_helpers_signature(&*self.isa),
            });
        }
        Ok(lane_bodies)
    }

    /// The symbol layout after `defined` imports: a function's lane body publishes under its own
    /// symbol, after the functions and the private regions; its fallback after the lane bodies.
    fn symbols(&self, defined: usize, lane_bodies: &[Option<ir::Function>]) -> Result<Symbols, String> {
        let first_definition = u32::try_from(defined).map_err(|_| "native symbol count exceeds u32")?;
        let region_count = (0..self.program.function_count())
            .try_fold(0usize, |count, id| count.checked_add(self.options.region_count(self.program.function(id).code.len())))
            .ok_or("native region count overflow")?;
        let body_base = (first_definition as usize)
            .checked_add(self.program.function_count())
            .and_then(|base| base.checked_add(region_count))
            .ok_or("native lane body symbol count overflow")?;
        let fallback_base = body_base.checked_add(lane_bodies.iter().flatten().count())
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
            let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
                NATIVE_DATA_NAMESPACE, self.function_id,
            ));
            builder.create_global_value(ir::GlobalValueData::Symbol {
                name: ir::ExternalName::user(name),
                offset: ir::immediates::Imm64::new(0),
                colocated: false,
                tls: false,
            })
        });
        let address = builder.ins().symbol_value(self.isa.pointer_type(), base);
        // Both callers bound the instruction count, so this offset is host-addressable.
        let offset = i64::try_from(pc * std::mem::size_of::<NativeJsOperands>())
            .expect("host addressable operand offset");
        let address = builder.ins().iadd_imm_s(address, offset);
        let pc = builder.ins().iconst(types::I64, pc as i64);
        let call = builder.ins().call(entry, &[context, pc, address, result]);
        builder.inst_results(call)[0]
    }
}

fn object_fallback_table(
    function: &Function, name: &str, isa: &dyn TargetIsa, operations: &[Option<u32>],
) -> Result<NativeData, String> {
    let pointer_bytes = usize::from(isa.pointer_bytes());
    let size = function.code.len().checked_mul(pointer_bytes).ok_or("native fallback table size overflow")?;
    let mut addresses = Vec::with_capacity(function.code.len());
    for (pc, packed) in function.code.iter().copied().enumerate() {
        let op = super::dispatch_fast::decoded_instruction(function, packed).op();
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

/// One exact semantic operation for a guarded native path's fallback.
fn lower_object_fallback(
    function: &Function, function_id: u32, table: u32, isa: &dyn TargetIsa,
) -> Result<ir::Function, String> {
    let count = u32::try_from(function.code.len()).map_err(|_| "native instruction count exceeds u32")?;
    let mut native = ir::Function::with_name_signature(
        ir::UserFuncName::user(NATIVE_FUNCTION_NAMESPACE, function_id),
        super::compiled::native_js_operation_signature(isa),
    );
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let params = builder.block_params(entry).to_vec();
    let pc = params[1];
    let valid_pc = builder.create_block();
    let invalid = builder.create_block();
    let valid = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, pc, i64::from(count));
    builder.ins().brif(valid, valid_pc, &[], invalid, &[]);
    builder.switch_to_block(valid_pc);
    let symbol = |builder: &mut FunctionBuilder<'_>, index| {
        let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
            NATIVE_DATA_NAMESPACE, index,
        ));
        let value = builder.create_global_value(ir::GlobalValueData::Symbol {
            name: ir::ExternalName::user(name), offset: ir::immediates::Imm64::new(0),
            colocated: false, tls: false,
        });
        builder.ins().symbol_value(isa.pointer_type(), value)
    };
    let table_base = symbol(&mut builder, table);
    let table_offset = builder.ins().imul_imm_u(pc, i64::from(isa.pointer_bytes()));
    let entry_address = builder.ins().iadd(table_base, table_offset);
    let entry = builder.ins().load(isa.pointer_type(), MemFlagsData::trusted(), entry_address, 0);
    let operands_base = symbol(&mut builder, function_id);
    let operands_offset = builder.ins().imul_imm_u(pc, std::mem::size_of::<NativeJsOperands>() as i64);
    let operands = builder.ins().iadd(operands_base, operands_offset);
    let signature = builder.import_signature(super::compiled::native_js_operation_signature(isa));
    let call = builder.ins().call_indirect(signature, entry, &[params[0], pc, operands, params[3]]);
    let status = builder.inst_results(call)[0];
    builder.ins().return_(&[status]);
    builder.switch_to_block(invalid);
    let status = builder.ins().iconst(types::I64, i64::from(super::compiled::NATIVE_JS_STOPPED));
    builder.ins().return_(&[status]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    Ok(native)
}

/// Bindings choose linker symbols or live JIT addresses; the graph and semantic
/// continuation protocol are identical in both publication modes.
pub(super) fn lower_body(
    sites: &[InstructionSite],
    function_id: u32,
    isa: &dyn TargetIsa,
    region: Option<Range<usize>>,
    mut operation: impl FnMut(&mut FunctionBuilder<'_>, &InstructionSite, ir::Value, ir::Value) -> ir::Value,
) -> Result<ir::Function, String> {
    u32::try_from(sites.len()).map_err(|_| "native instruction count exceeds u32")?;
    let is_region = region.is_some();
    let range = region.unwrap_or(0..sites.len());
    if range.start > range.end || range.end > sites.len() {
        return Err("invalid native region bounds".into());
    }
    let count =
        u32::try_from(range.len()).map_err(|_| "native region instruction count exceeds u32")?;
    let mut native = ir::Function::with_name_signature(
        ir::UserFuncName::user(NATIVE_FUNCTION_NAMESPACE, function_id),
        super::compiled::native_entry_signature(isa.pointer_type(), isa.default_call_conv()),
    );
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let context = builder.block_params(entry)[0];
    let result = builder.block_params(entry)[4];
    let start_pc = if is_region {
        builder.block_params(entry)[REGION_START_PC_PARAMETER]
    } else {
        super::compiled::native_js_start_pc(&mut builder, context)
    };
    let resume = builder.create_block();
    let pc = builder.append_block_param(resume, types::I64);
    let select = builder.create_block();
    let selector = builder.append_block_param(select, types::I32);
    let invalid = builder.create_block();
    let invalid_pc = builder.append_block_param(invalid, types::I64);
    let stopped = builder.create_block();
    let blocks: Vec<_> = (0..count).map(|_| builder.create_block()).collect();
    builder.ins().jump(resume, &[ir::BlockArg::Value(start_pc)]);

    for (offset, block) in blocks.iter().copied().enumerate() {
        let site = &sites[range.start + offset];
        builder.switch_to_block(block);
        if let Continuation::Static(target) = site.continuation {
            match target.checked_sub(range.start).and_then(|offset| blocks.get(offset)) {
                Some(block) => {
                    builder.ins().jump(*block, &[]);
                }
                None => {
                    let target = builder.ins().iconst(types::I64, target as i64);
                    builder.ins().jump(resume, &[ir::BlockArg::Value(target)]);
                }
            }
            continue;
        }
        let status = operation(&mut builder, site, context, result);
        let continued = builder.ins().icmp_imm_u(IntCC::Equal, status, 0);
        let successor = builder.create_block();
        builder.ins().brif(continued, successor, &[], stopped, &[]);
        builder.switch_to_block(successor);
        let next = builder
            .ins()
            .load(types::I64, MemFlagsData::trusted(), result, 0);
        if site.continuation == Continuation::Fallthrough
            && let Some(block) = blocks.get(offset + 1)
        {
            let fallthrough = builder
                .ins()
                .icmp_imm_u(IntCC::Equal, next, (site.pc + 1) as i64);
            builder.ins().brif(
                fallthrough,
                *block,
                &[],
                resume,
                &[ir::BlockArg::Value(next)],
            );
        } else {
            builder.ins().jump(resume, &[ir::BlockArg::Value(next)]);
        }
    }
    builder.switch_to_block(resume);
    // Check the full PC before narrowing it for br_table. A high word must not
    // alias a valid instruction and replay guest effects.
    let offset = builder.ins().iadd_imm_s(pc, -(range.start as i64));
    let in_range = builder
        .ins()
        .icmp_imm_u(IntCC::UnsignedLessThan, offset, i64::from(count));
    let index = builder.ins().ireduce(types::I32, offset);
    builder.ins().brif(
        in_range,
        select,
        &[ir::BlockArg::Value(index)],
        invalid,
        &[ir::BlockArg::Value(pc)],
    );
    builder.switch_to_block(select);
    let default = builder
        .func
        .dfg
        .block_call(invalid, &[ir::BlockArg::Value(pc)]);
    let cases: Vec<_> = blocks
        .iter()
        .map(|block| builder.func.dfg.block_call(*block, &[]))
        .collect();
    let table = builder.create_jump_table(ir::JumpTableData::new(default, &cases));
    builder.ins().br_table(selector, table);
    builder.switch_to_block(invalid);
    builder
        .ins()
        .store(MemFlagsData::trusted(), invalid_pc, result, 0);
    let status = builder.ins().iconst(
        types::I32,
        i64::from(if is_region {
            NATIVE_JS_REGION_EXIT
        } else {
            super::compiled::NATIVE_JS_INVALID_PC
        }),
    );
    builder.ins().return_(&[status]);
    builder.switch_to_block(stopped);
    let status = builder
        .ins()
        .iconst(types::I32, i64::from(super::compiled::NATIVE_JS_STOPPED));
    builder.ins().return_(&[status]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    Ok(native)
}

/// Private regions reuse the native ABI, carrying their starting PC in its
/// otherwise unused memory-base word. Guest arguments remain in the VM frame.
pub(super) fn region_call_arguments(
    builder: &mut FunctionBuilder<'_>,
    isa: &dyn TargetIsa,
    context: ir::Value,
    pc: ir::Value,
    result: ir::Value,
) -> [ir::Value; 5] {
    let length = builder.ins().iconst(types::I64, 0);
    let arguments = builder.ins().iconst(isa.pointer_type(), 0);
    [context, pc, length, arguments, result]
}

pub(super) fn lower_region_dispatch(
    function_id: u32,
    instruction_count: usize,
    width: usize,
    region_count: usize,
    isa: &dyn TargetIsa,
    mut call: impl FnMut(&mut FunctionBuilder<'_>, usize, ir::Value, ir::Value, ir::Value) -> ir::Value,
) -> Result<ir::Function, String> {
    let count =
        u32::try_from(instruction_count).map_err(|_| "native instruction count exceeds u32")?;
    if width == 0 || instruction_count.div_ceil(width) != region_count {
        return Err("invalid native region plan".into());
    }
    let mut native = ir::Function::with_name_signature(
        ir::UserFuncName::user(NATIVE_FUNCTION_NAMESPACE, function_id),
        super::compiled::native_entry_signature(isa.pointer_type(), isa.default_call_conv()),
    );
    let mut context_builder = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context_builder);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let context = builder.block_params(entry)[0];
    let result = builder.block_params(entry)[4];
    let start = super::compiled::native_js_start_pc(&mut builder, context);
    let resume = builder.create_block();
    let pc = builder.append_block_param(resume, types::I64);
    let select = builder.create_block();
    let selector = builder.append_block_param(select, types::I32);
    let invalid = builder.create_block();
    let invalid_pc = builder.append_block_param(invalid, types::I64);
    let stopped = builder.create_block();
    let status = builder.append_block_param(stopped, types::I32);
    let blocks: Vec<_> = (0..region_count).map(|_| builder.create_block()).collect();
    builder.ins().jump(resume, &[ir::BlockArg::Value(start)]);
    for (index, block) in blocks.iter().copied().enumerate() {
        builder.switch_to_block(block);
        let status = call(&mut builder, index, context, pc, result);
        let continued =
            builder
                .ins()
                .icmp_imm_u(IntCC::Equal, status, i64::from(NATIVE_JS_REGION_EXIT));
        let next = builder.create_block();
        builder.ins().brif(
            continued,
            next,
            &[],
            stopped,
            &[ir::BlockArg::Value(status)],
        );
        builder.switch_to_block(next);
        let next_pc = builder
            .ins()
            .load(types::I64, MemFlagsData::trusted(), result, 0);
        builder.ins().jump(resume, &[ir::BlockArg::Value(next_pc)]);
    }
    builder.switch_to_block(resume);
    let in_range = builder
        .ins()
        .icmp_imm_u(IntCC::UnsignedLessThan, pc, i64::from(count));
    let divisor = builder.ins().iconst(types::I64, width as i64);
    let region = builder.ins().udiv(pc, divisor);
    let index = builder.ins().ireduce(types::I32, region);
    builder.ins().brif(
        in_range,
        select,
        &[ir::BlockArg::Value(index)],
        invalid,
        &[ir::BlockArg::Value(pc)],
    );
    builder.switch_to_block(select);
    let default = builder
        .func
        .dfg
        .block_call(invalid, &[ir::BlockArg::Value(pc)]);
    let cases: Vec<_> = blocks
        .iter()
        .map(|block| builder.func.dfg.block_call(*block, &[]))
        .collect();
    let table = builder.create_jump_table(ir::JumpTableData::new(default, &cases));
    builder.ins().br_table(selector, table);
    builder.switch_to_block(invalid);
    builder
        .ins()
        .store(MemFlagsData::trusted(), invalid_pc, result, 0);
    let invalid_status = builder
        .ins()
        .iconst(types::I32, i64::from(super::compiled::NATIVE_JS_INVALID_PC));
    builder.ins().return_(&[invalid_status]);
    builder.switch_to_block(stopped);
    builder.ins().return_(&[status]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
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
