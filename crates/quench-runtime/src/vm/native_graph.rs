//! Native control flow over statically selected shared semantic operations.

use super::native_object::{
    NATIVE_DATA_NAMESPACE, NATIVE_FUNCTION_NAMESPACE, NativeData, NativeFunction,
    NativeFunctionAddress,
};
use super::native_operands::NativeJsOperands;
use super::specialize::{CallThreaded, CallThreadedExtension, Continuation, InstructionSite, Specializer};
use crate::bytecode::{Op, ProgramKind, ResidualProgram};
use cranelift_codegen::{
    ir::{self, InstBuilder, MemFlagsData, condcodes::IntCC, types},
    isa::TargetIsa,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use std::{num::NonZeroUsize, ops::Range};

// Bounds the working CFG, not the source function. Large unpartitioned native
// graphs exhausted 14 GiB on a 50 KiB input; callers may choose any nonzero budget.
const DEFAULT_REGION_INSTRUCTION_BUDGET: NonZeroUsize = NonZeroUsize::new(1024).unwrap();
const NATIVE_JS_REGION_EXIT: u32 = super::compiled::NATIVE_JS_INVALID_PC + 1;
// The shared entry ABI's memory-base word is unused by JavaScript bodies.
const REGION_START_PC_PARAMETER: usize = 1;

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
    pub(super) fn regions(self, count: usize) -> Option<Vec<Range<usize>>> {
        let width = self.region_instruction_budget.get();
        (count > width).then(|| {
            (0..count)
                .step_by(width)
                .map(|start| start..start.saturating_add(width).min(count))
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
    CallThreaded(ObjectExtension { isa, namespace, options, functions: Vec::new() }).specialize(program)
}

/// The call-threaded generating extension for linked objects: each semantic
/// entry is an imported symbol, each operand record is object data, and the
/// linker binds both.
struct ObjectExtension<'n> {
    isa: std::sync::Arc<dyn TargetIsa>,
    namespace: &'n str,
    options: NativeJsCodegenOptions,
    functions: Vec<Vec<InstructionSite>>,
}

impl CallThreadedExtension for ObjectExtension<'_> {
    type Residual = NativeJsModule;

    fn function(&mut self, _function_id: u32, sites: Vec<InstructionSite>) -> Result<(), String> {
        self.functions.push(sites);
        Ok(())
    }

    fn finish(self) -> Result<NativeJsModule, String> {
        let Self { isa, namespace, options, functions: bodies } = self;
        let mut functions = Vec::new();
        let abi_function = 0;
        functions.push(NativeFunction::Import {
            name: crate::NATIVE_JS_ABI_SYMBOL.into(),
            signature: ir::Signature::new(isa.default_call_conv()),
        });
        let mut operations = vec![None; Op::COUNT];
        for id in 0..bodies.len() {
            let body = &bodies[id];
            for op in body.iter().filter(|site| site.calls_semantic_entry()).map(|site| site.op) {
                if operations[op as usize].is_none() {
                    let index = u32::try_from(functions.len())
                        .map_err(|_| "native symbol count exceeds u32")?;
                    operations[op as usize] = Some(index);
                    functions.push(NativeFunction::Import {
                        name: format!(
                            "{}{}",
                            crate::bytecode::NATIVE_JS_OPERATION_PREFIX,
                            Op::NAMES[op as usize]
                        ),
                        signature: super::compiled::native_js_operation_signature(&*isa),
                    });
                }
            }
        }
        let first_definition =
            u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?;
        let mut data = Vec::with_capacity(bodies.len());
        let mut region_definitions = Vec::new();
        for id in 0..bodies.len() {
            let body = &bodies[id];
            let function_id = u32::try_from(id).map_err(|_| "native function count exceeds u32")?;
            let byte_len = body
                .len()
                .checked_mul(std::mem::size_of::<NativeJsOperands>())
                .ok_or("native operand data size overflow")?;
            let mut bytes = Vec::with_capacity(byte_len);
            for site in body {
                site.operands.append_native_bytes(&mut bytes);
            }
            let native = if let Some(regions) = options.regions(body.len()) {
                let region_base = (first_definition as usize)
                    .checked_add(bodies.len())
                    .and_then(|base| base.checked_add(region_definitions.len()))
                    .ok_or("native symbol count overflow")?;
                let region_end = region_base
                    .checked_add(regions.len())
                    .ok_or("native symbol count overflow")?;
                u32::try_from(region_end).map_err(|_| "native symbol count exceeds u32")?;
                for (index, region) in regions.iter().cloned().enumerate() {
                    region_definitions.push(NativeFunction::Definition {
                        name: format!("{namespace}_function_{id}_region_{index}"),
                        function: lower_object_body(
                            body,
                            function_id,
                            &*isa,
                            &operations,
                            Some(region),
                        )?,
                    });
                }
                let mut signature = None;
                lower_region_dispatch(
                    function_id,
                    body.len(),
                    options.region_instruction_budget.get(),
                    regions.len(),
                    &*isa,
                    |builder, index, context, pc, result| {
                        let signature = *signature.get_or_insert_with(|| {
                            builder.import_signature(super::compiled::native_entry_signature(
                                isa.pointer_type(),
                                isa.default_call_conv(),
                            ))
                        });
                        let symbol =
                            u32::try_from(region_base + index).expect("validated native region symbol");
                        let name =
                            builder
                                .func
                                .declare_imported_user_function(ir::UserExternalName::new(
                                    NATIVE_FUNCTION_NAMESPACE,
                                    symbol,
                                ));
                        let callee = builder.import_function(ir::ExtFuncData {
                            name: ir::ExternalName::user(name),
                            signature,
                            colocated: false,
                            patchable: false,
                        });
                        let arguments = region_call_arguments(builder, &*isa, context, pc, result);
                        let call = builder.ins().call(callee, &arguments);
                        builder.inst_results(call)[0]
                    },
                )?
            } else {
                lower_object_body(body, function_id, &*isa, &operations, None)?
            };
            data.push(NativeData {
                name: format!("{namespace}_operands_{id}"),
                bytes,
                alignment: std::mem::align_of::<NativeJsOperands>() as u64,
                writable: false,
                function_addresses: Vec::new(),
            });
            functions.push(NativeFunction::Definition {
                name: format!("{namespace}_function_{id}"),
                function: native,
            });
        }
        functions.extend(region_definitions);
        u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?;
        let pointer_bytes = usize::from(isa.pointer_bytes());
        let words = bodies
            .len()
            .checked_add(crate::NATIVE_JS_IMAGE_HEADER_WORDS)
            .ok_or("native entry table size overflow")?;
        let bytes = words
            .checked_mul(pointer_bytes)
            .ok_or("native entry table size overflow")?;
        let mut addresses = Vec::with_capacity(words);
        addresses.push(NativeFunctionAddress {
            offset: 0,
            function: abi_function,
        });
        for id in 0..bodies.len() {
            let word = id + crate::NATIVE_JS_IMAGE_HEADER_WORDS;
            let offset = u32::try_from(word * pointer_bytes)
                .map_err(|_| "native entry table exceeds relocation offsets")?;
            let function = first_definition
                .checked_add(u32::try_from(id).map_err(|_| "native function count exceeds u32")?)
                .ok_or("native function symbol count exceeds u32")?;
            addresses.push(NativeFunctionAddress { offset, function });
        }
        let entry_table = data.len();
        data.push(NativeData {
            name: format!("{namespace}_entries"),
            bytes: vec![0; bytes],
            alignment: pointer_bytes as u64,
            writable: false,
            function_addresses: addresses,
        });
        Ok(NativeJsModule {
            functions,
            data,
            entry_table,
        })
    }
}

fn lower_object_body(
    sites: &[InstructionSite],
    function_id: u32,
    isa: &dyn TargetIsa,
    operations: &[Option<u32>],
    region: Option<Range<usize>>,
) -> Result<ir::Function, String> {
    let mut entries = vec![None; Op::COUNT];
    let mut operand_base = None;
    lower_body(
        sites,
        function_id,
        isa,
        region,
        |builder, site, context, result| {
            let (op, pc) = (site.op, site.pc);
            let entry = *entries[op as usize].get_or_insert_with(|| {
                let signature =
                    builder.import_signature(super::compiled::native_js_operation_signature(isa));
                let name = builder
                    .func
                    .declare_imported_user_function(ir::UserExternalName::new(
                        NATIVE_FUNCTION_NAMESPACE,
                        operations[op as usize].expect("collected operation"),
                    ));
                builder.import_function(ir::ExtFuncData {
                    name: ir::ExternalName::user(name),
                    signature,
                    colocated: false,
                    patchable: false,
                })
            });
            let base = *operand_base.get_or_insert_with(|| {
                let name = builder
                    .func
                    .declare_imported_user_function(ir::UserExternalName::new(
                        NATIVE_DATA_NAMESPACE,
                        function_id,
                    ));
                builder.create_global_value(ir::GlobalValueData::Symbol {
                    name: ir::ExternalName::user(name),
                    offset: ir::immediates::Imm64::new(0),
                    colocated: false,
                    tls: false,
                })
            });
            let address = builder.ins().symbol_value(isa.pointer_type(), base);
            // lower_body bounds the instruction count; operand offsets are host-addressable.
            let offset = i64::try_from(pc * std::mem::size_of::<NativeJsOperands>())
                .expect("host addressable operand offset");
            let address = builder.ins().iadd_imm_s(address, offset);
            let pc = builder.ins().iconst(types::I64, pc as i64);
            let call = builder.ins().call(entry, &[context, pc, address, result]);
            builder.inst_results(call)[0]
        },
    )
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
            assert_eq!(definitions.len(), program.function_count());
            assert!(definitions.contains(&"graph_function_0"), "root must execute natively");
            let object = super::super::native_object::emit_native_module("graph", module.functions, module.data)
                .expect("all function and operand relocations resolve");
            assert!(!object.is_empty());
        }
    }
}
