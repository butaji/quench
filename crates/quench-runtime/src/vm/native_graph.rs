//! Native control flow over statically selected shared semantic operations.

use super::native_object::{
    NATIVE_DATA_NAMESPACE, NATIVE_FUNCTION_NAMESPACE, NativeData, NativeFunction,
};
use super::native_operands::NativeJsOperands;
use crate::bytecode::{ControlFlowLayout, Function, Op, ProgramKind, ResidualProgram};
use cranelift_codegen::{
    ir::{self, InstBuilder, MemFlagsData, condcodes::IntCC, types},
    isa::TargetIsa,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

/// Link inputs for all functions in one source program, including its entry.
/// Semantic imports name compiler-owned operations, never process addresses.
pub struct NativeJsModule {
    pub functions: Vec<NativeFunction>,
    pub data: Vec<NativeData>,
}

/// Lower every function before execution. Deferred function bodies are resolved
/// through the program's existing authority, just as they are for the JIT.
pub fn lower_native_js_module(
    program: &ResidualProgram,
    namespace: &str,
) -> Result<NativeJsModule, String> {
    if program.kind == ProgramKind::Wasm {
        return Err("JavaScript native graph requires a JS or TS program".into());
    }
    program.validate_unless_proven()?;
    let isa = super::jit_backend::target_isa_for(
        "speed",
        super::jit_backend::CodePublication::RelocatableObject,
    )
    .ok_or("native target unavailable")?;
    let mut functions = Vec::new();
    let mut operations = vec![None; Op::COUNT];
    for id in 0..program.function_count() {
        let body = program.function(id);
        for packed in body.code.iter().copied() {
            let op = super::dispatch_fast::decoded_instruction(body, packed).op();
            if operations[op as usize].is_none() {
                let index = u32::try_from(functions.len())
                    .map_err(|_| "native symbol count exceeds u32")?;
                operations[op as usize] = Some(index);
                functions.push(NativeFunction::Import {
                    name: format!("quench_js_op_{}", Op::NAMES[op as usize]),
                    signature: super::compiled::native_js_operation_signature(&*isa),
                });
            }
        }
    }
    let mut data = Vec::with_capacity(program.function_count());
    for id in 0..program.function_count() {
        let body = program.function(id);
        let function_id = u32::try_from(id).map_err(|_| "native function count exceeds u32")?;
        let byte_len = body
            .code
            .len()
            .checked_mul(std::mem::size_of::<NativeJsOperands>())
            .ok_or("native operand data size overflow")?;
        let mut bytes = Vec::with_capacity(byte_len);
        for packed in body.code.iter().copied() {
            let instruction = super::dispatch_fast::decoded_instruction(body, packed);
            NativeJsOperands::new(
                instruction.a(),
                instruction.b(),
                instruction.c(),
                instruction.imm(),
            )
            .append_native_bytes(&mut bytes);
        }
        let mut entries = vec![None; Op::COUNT];
        let mut operand_base = None;
        let native =
            lower_function(
                body,
                function_id,
                &*isa,
                |builder, op, pc, context, result| {
                    let entry = *entries[op as usize].get_or_insert_with(|| {
                        let signature = builder.import_signature(
                            super::compiled::native_js_operation_signature(&*isa),
                        );
                        let name =
                            builder
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
                    let base =
                        *operand_base.get_or_insert_with(|| {
                            let name = builder.func.declare_imported_user_function(
                                ir::UserExternalName::new(NATIVE_DATA_NAMESPACE, function_id),
                            );
                            builder.create_global_value(ir::GlobalValueData::Symbol {
                                name: ir::ExternalName::user(name),
                                offset: ir::immediates::Imm64::new(0),
                                colocated: false,
                                tls: false,
                            })
                        });
                    let address = builder.ins().symbol_value(isa.pointer_type(), base);
                    // lower_function bounds the instruction count; byte_len was checked above.
                    let offset = i64::try_from(pc * std::mem::size_of::<NativeJsOperands>())
                        .expect("host addressable operand offset");
                    let address = builder.ins().iadd_imm_s(address, offset);
                    let pc = builder.ins().iconst(types::I64, pc as i64);
                    let call = builder.ins().call(entry, &[context, pc, address, result]);
                    builder.inst_results(call)[0]
                },
            )?;
        data.push(NativeData {
            name: format!("{namespace}_operands_{id}"),
            bytes,
            alignment: std::mem::align_of::<NativeJsOperands>() as u64,
            writable: false,
        });
        functions.push(NativeFunction::Definition {
            name: format!("{namespace}_function_{id}"),
            function: native,
        });
    }
    Ok(NativeJsModule { functions, data })
}

/// Bindings choose linker symbols or live JIT addresses; the graph and semantic
/// continuation protocol are identical in both publication modes.
pub(super) fn lower_function(
    function: &Function,
    function_id: u32,
    isa: &dyn TargetIsa,
    mut operation: impl FnMut(&mut FunctionBuilder<'_>, Op, usize, ir::Value, ir::Value) -> ir::Value,
) -> Result<ir::Function, String> {
    let count =
        u32::try_from(function.code.len()).map_err(|_| "native instruction count exceeds u32")?;
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
    let start_pc = super::compiled::native_js_start_pc(&mut builder, context);
    let resume = builder.create_block();
    let pc = builder.append_block_param(resume, types::I64);
    let select = builder.create_block();
    let selector = builder.append_block_param(select, types::I32);
    let invalid = builder.create_block();
    let invalid_pc = builder.append_block_param(invalid, types::I64);
    let stopped = builder.create_block();
    let blocks: Vec<_> = (0..count).map(|_| builder.create_block()).collect();
    builder.ins().jump(resume, &[ir::BlockArg::Value(start_pc)]);

    for (pc, block) in blocks.iter().copied().enumerate() {
        builder.switch_to_block(block);
        let instruction = super::dispatch_fast::decoded_instruction(function, function.code[pc]);
        let status = operation(&mut builder, instruction.op(), pc, context, result);
        let continued = builder.ins().icmp_imm_u(IntCC::Equal, status, 0);
        let successor = builder.create_block();
        builder.ins().brif(continued, successor, &[], stopped, &[]);
        builder.switch_to_block(successor);
        let next = builder
            .ins()
            .load(types::I64, MemFlagsData::trusted(), result, 0);
        if instruction.op().control_flow_layout() == ControlFlowLayout::Fallthrough
            && let Some(block) = blocks.get(pc + 1)
        {
            let fallthrough = builder
                .ins()
                .icmp_imm_u(IntCC::Equal, next, (pc + 1) as i64);
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
    let in_range = builder
        .ins()
        .icmp_imm_u(IntCC::UnsignedLessThan, pc, i64::from(count));
    let index = builder.ins().ireduce(types::I32, pc);
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
    let status = builder
        .ins()
        .iconst(types::I32, i64::from(super::compiled::NATIVE_JS_INVALID_PC));
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
