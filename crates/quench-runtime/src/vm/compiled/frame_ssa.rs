//! Canonical frame homes are recovery edges, not the native register file.
//!
//! Resume entries admit canonical tagged inputs once. Native definitions and
//! CFG joins carry the explicit generic JavaScript type; semantic operations
//! retain their narrower Number checks. Only an effect/recovery boundary
//! materializes the live state. Guest callouts resume through fresh entries,
//! so aliased parameters and references moved by GC are never cached across one.

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use cranelift_codegen::ir::InstBuilderBase;

pub(super) struct FrameFlow {
    registers: ir::Value,
    count: u16,
    roots: BTreeMap<ir::Inst, u16>,
    residuals: BTreeMap<ir::Inst, BTreeSet<u16>>,
}

impl FrameFlow {
    pub(super) fn new(registers: ir::Value, count: u16) -> Self {
        Self { registers, count, roots: BTreeMap::new(), residuals: BTreeMap::new() }
    }

    /// This call targets a verified scalar residual: canonical writes, no GC.
    #[cfg(feature = "aot")]
    pub(super) fn residual(&mut self, call: ir::Inst, frame: &[Register]) {
        self.residuals.insert(call, frame.iter().copied().collect());
    }

    pub(super) fn resume(&mut self, builder: &mut FunctionBuilder<'_>,
        live: Option<&crate::compile::liveness::RegisterMasks>, pc: usize) {
        for register in 0..self.count {
            let value = if live.is_none_or(|live| live.is_live(pc, register)) {
                builder.ins().load(types::I64, MemFlagsData::trusted(), self.registers, offset(register))
            } else { builder.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64) };
            let ir::ValueDef::Result(instruction, _) = builder.func.dfg.value_def(value) else { unreachable!() };
            self.roots.insert(instruction, register);
        }
    }

    pub(super) fn promote(self, native: &mut ir::Function, callout: ir::Value,
        live: Option<&crate::compile::liveness::RegisterMasks>, config: cranelift_codegen::isa::TargetFrontendConfig) {
        let records: Vec<_> = native.layout.blocks().map(|block| {
            let instructions: Vec<_> = native.layout.block_insts(block).collect();
            (block, instructions)
        }).collect();
        // Reject unknown effects before changing the graph. Registered scalar
        // residuals cannot move frame buffers.
        // Other address roots belong to disjoint locals/result/VM heap storage
        // under the native-entry ABI. The canonical register pointer must not
        // escape into another view or be written as a value.
        for (_, instructions) in &records {
            for instruction in instructions {
                let data = &native.dfg.insts[*instruction];
                if data.opcode().is_call() && !self.known_call(native, *instruction, callout) {
                    return;
                }
                if native.dfg.inst_args(*instruction).iter().any(|value|
                    native.dfg.resolve_aliases(*value) == native.dfg.resolve_aliases(self.registers))
                    && !data.opcode().is_call()
                {
                    let Some((_, value, store)) = self.cell(native, *instruction) else { return };
                    if store && native.dfg.resolve_aliases(value) == native.dfg.resolve_aliases(self.registers) { return; }
                }
            }
        }
        let mut dirty = BTreeMap::<ir::Block, BTreeSet<u16>>::new();
        loop {
            let mut changed = false;
            for (block, instructions) in &records {
                let mut current = dirty.get(block).cloned().unwrap_or_default();
                for instruction in instructions {
                    if self.roots.contains_key(instruction) { current.clear(); }
                    if let Some((register, _, store)) = self.cell(native, *instruction) {
                        if store { current.insert(register); }
                    }
                    if native.dfg.insts[*instruction].opcode().is_call() {
                        match self.residuals.get(instruction) {
                            Some(frame) => current.retain(|register| !frame.contains(register)),
                            None => current.clear(),
                        }
                    }
                }
                let Some(last) = instructions.last() else { continue };
                for destination in native.dfg.insts[*last].branch_destination(&native.dfg.jump_tables, &native.dfg.exception_tables) {
                    let incoming = dirty.entry(destination.block(&native.dfg.value_lists)).or_default();
                    let size = incoming.len();
                    incoming.extend(&current);
                    changed |= incoming.len() != size;
                }
            }
            if !changed { break; }
        }
        let parameters: Vec<_> = records.iter().map(|(block, _)| native.dfg.block_params(*block).to_vec()).collect();
        for (block, _) in &records { native.layout.remove_block_and_insts(*block); }
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(native, &mut context);
        let blocks: BTreeMap<_, _> = records.iter().map(|(block, _)| (*block, builder.create_block())).collect();
        for ((block, _), parameters) in records.iter().zip(&parameters) {
            builder.func.dfg.detach_block_params(*block);
            for parameter in parameters {
                let next = builder.append_block_param(blocks[block], builder.func.dfg.value_type(*parameter));
                builder.func.dfg.change_to_alias(*parameter, next);
            }
        }
        // All frame SSA values have semantic ANY/Tagged. Narrower types are
        // established by the shared operation's guards, never by these homes.
        let fact = typed_ir::Fact::tagged(typed_ir::BindingTime::Dynamic, typed_ir::Proof::Definition)
            .verified().expect("canonical frame definition");
        assert_eq!(fact.representation, typed_ir::Representation::Tagged);
        let variables: Vec<_> = (0..self.count).map(|_| builder.declare_var(types::I64)).collect();
        for (position, (block, instructions)) in records.iter().enumerate() {
            builder.switch_to_block(blocks[block]);
            if position == 0 {
                let undefined = builder.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
                for variable in &variables { builder.def_var(*variable, undefined); }
            }
            let mut current = dirty.get(block).cloned().unwrap_or_default();
            for instruction in instructions {
                let data = builder.func.dfg.insts[*instruction].clone();
                let results = builder.func.dfg.inst_results(*instruction).to_vec();
                let control = builder.func.dfg.ctrl_typevar(*instruction);
                let cell = self.cell(builder.func, *instruction);
                let root = self.roots.get(instruction).copied();
                let pc = self.recovery_pc(builder.func, *instruction, callout);
                let residual = self.residuals.get(instruction);
                if root.is_some() { current.clear(); }
                builder.func.dfg.clear_results(*instruction);
                // Branch argument lists are owned by the replacement. SSA
                // sealing may resize them, so detached instructions must not
                // retain a second handle to the same list.
                builder.func.dfg.insts[*instruction] = ir::InstructionData::UnaryImm {
                    opcode: ir::Opcode::Iconst, imm: 0.into(),
                };
                if root.is_none() && let Some((register, value, store)) = cell {
                    if store {
                        builder.def_var(variables[usize::from(register)], value);
                        current.insert(register);
                    } else {
                        let value = builder.use_var(variables[usize::from(register)]);
                        builder.func.dfg.change_to_alias(results[0], value);
                    }
                    continue;
                }
                let is_call = data.opcode().is_call();
                let is_return = data.opcode() == ir::Opcode::Return;
                if is_call || is_return {
                    for register in &current {
                        if residual.is_some_and(|frame| !frame.contains(register)) { continue; }
                        if pc.is_some_and(|pc| live.is_some_and(|live| !live.is_live(pc, *register))) { continue; }
                        let value = builder.use_var(variables[usize::from(*register)]);
                        builder.ins().store(MemFlagsData::trusted(), value, self.registers, offset(*register));
                    }
                    match residual {
                        Some(frame) => current.retain(|register| !frame.contains(register)),
                        None => current.clear(),
                    }
                }
                let mut data = data;
                let dfg = &mut builder.func.dfg;
                for destination in data.branch_destination_mut(&mut dfg.jump_tables, &mut dfg.exception_tables) {
                    if let Some(block) = blocks.get(&destination.block(&dfg.value_lists)) {
                        destination.set_block(*block, &mut dfg.value_lists);
                    }
                }
                let (next, _) = builder.ins().build(data, control);
                builder.func.dfg.clear_results(next);
                let input = results.first().copied();
                builder.func.dfg.make_inst_results_reusing(next, control, results.into_iter().map(Some));
                if let Some(register) = root {
                    builder.def_var(variables[usize::from(register)], input.expect("resume input"));
                }
                if let Some(frame) = residual {
                    // A scalar residual may update canonical homes on either
                    // success or deopt, but cannot move or release the buffers.
                    for register in frame {
                        let value = builder.ins().load(types::I64, MemFlagsData::trusted(), self.registers, offset(*register));
                        builder.def_var(variables[usize::from(*register)], value);
                    }
                }
            }
        }
        builder.seal_all_blocks();
        builder.finalize(config);
    }

    fn cell(&self, native: &ir::Function, instruction: ir::Inst) -> Option<(u16, ir::Value, bool)> {
        let data = &native.dfg.insts[instruction];
        let (base, displacement, value, store) = match data {
            ir::InstructionData::Load { arg, offset, .. } if data.opcode() == ir::Opcode::Load =>
                (*arg, i32::from(*offset), native.dfg.first_result(instruction), false),
            ir::InstructionData::Store { args, offset, .. } if data.opcode() == ir::Opcode::Store =>
                (args[1], i32::from(*offset), args[0], true),
            _ => return None,
        };
        if native.dfg.resolve_aliases(base) != native.dfg.resolve_aliases(self.registers)
            || native.dfg.value_type(value) != types::I64 { return None; }
        let displacement = usize::try_from(displacement).ok()?;
        let width = std::mem::size_of::<Value>();
        if !displacement.is_multiple_of(width) { return None; }
        let register = u16::try_from(displacement / width).ok()?;
        (register < self.count).then_some((register, value, store))
    }

    fn recovery_pc(&self, native: &ir::Function, instruction: ir::Inst, callout: ir::Value) -> Option<usize> {
        let opcode = native.dfg.insts[instruction].opcode();
        let args = native.dfg.inst_args(instruction);
        let (value, base) = match opcode {
            ir::Opcode::CallIndirect if native.dfg.resolve_aliases(args[0]) == native.dfg.resolve_aliases(callout) => (args[2], 0),
            ir::Opcode::Return => (args[0], i64::from(JS_GENERAL_RETURN_STATUS_BASE)),
            _ => return None,
        };
        let ir::ValueDef::Result(constant, _) = native.dfg.value_def(native.dfg.resolve_aliases(value)) else { return None };
        let ir::InstructionData::UnaryImm { imm, .. } = native.dfg.insts[constant] else { return None };
        usize::try_from(imm.bits().checked_sub(base)?).ok()
    }

    fn known_call(&self, native: &ir::Function, instruction: ir::Inst, callout: ir::Value) -> bool {
        // Effect identity does not require a static PC: bounded native entry
        // carries a dynamic PC through the same fixed semantic operation table.
        (native.dfg.insts[instruction].opcode() == ir::Opcode::CallIndirect
            && native.dfg.resolve_aliases(native.dfg.inst_args(instruction)[0]) == native.dfg.resolve_aliases(callout))
            || self.residuals.contains_key(&instruction)
    }
}

fn offset(register: u16) -> i32 { i32::from(register) * std::mem::size_of::<Value>() as i32 }

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn mutate_frame(frame: u64, _pc: u64, _args: u64, result: *mut u64) -> u64 {
        // SAFETY: the test's native caller supplies its live two-cell frame.
        unsafe {
            let frame = frame as *mut u64;
            frame.write(100);
            result.write(0);
        }
        0
    }

    #[cfg(feature = "aot")]
    unsafe extern "C" fn scalar_write(_context: u64, frame: *mut u64, _unused: u64, _entry: u64, result: *mut u64) -> u32 {
        // SAFETY: the test binds a one-cell scalar footprint in its live frame.
        unsafe { frame.write(100); result.write(0); }
        0
    }

    #[test]
    #[cfg(feature = "aot")]
    fn scalar_effect_preserves_uncommitted_values_outside_its_footprint() {
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 2]);
        signature.returns.push(AbiParam::new(types::I64));
        let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut native, &mut context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let parameters = builder.block_params(entry).to_vec();
        let registers = parameters[0];
        let result = parameters[1];
        let callout = builder.ins().iconst(types::I64, 0);
        let mut flow = FrameFlow::new(registers, 2);
        flow.resume(&mut builder, None, 0);
        let kept = builder.ins().iconst(types::I64, 42);
        builder.ins().store(MemFlagsData::trusted(), kept, registers, offset(1));
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 5]);
        signature.returns.push(AbiParam::new(types::I32));
        let signature = builder.import_signature(signature);
        let target = builder.ins().iconst(types::I64, scalar_write as *const () as usize as i64);
        let zero = builder.ins().iconst(types::I64, 0);
        let call = builder.ins().call_indirect(signature, target, &[zero, registers, zero, zero, result]);
        flow.residual(call, &[0]);
        let left = builder.ins().load(types::I64, MemFlagsData::trusted(), registers, 0);
        let right = builder.ins().load(types::I64, MemFlagsData::trusted(), registers, offset(1));
        let sum = builder.ins().iadd(left, right);
        builder.ins().return_(&[sum]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        flow.promote(&mut native, callout, None, isa.frontend_config());
        let code = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: the native frame and scalar residual satisfy the declared footprint.
        let run = unsafe { std::mem::transmute::<*mut u8,
            unsafe extern "C" fn(*mut u64, *mut u64) -> u64>(code.pointer()) };
        let mut frame = [41, 7];
        let mut result = 0;
        assert_eq!(unsafe { run(frame.as_mut_ptr(), &mut result) }, 142);
        assert_eq!(frame, [100, 42]);
    }

    #[test]
    fn register_values_cross_cfg_backedges_without_canonical_spills() {
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.push(AbiParam::new(types::I64));
        signature.returns.push(AbiParam::new(types::I64));
        let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut native, &mut context);
        let entry = builder.create_block();
        let header = builder.create_block();
        let body = builder.create_block();
        let done = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let registers = builder.block_params(entry)[0];
        let callout = builder.ins().iconst(types::I64, 0);
        let mut flow = FrameFlow::new(registers, 2);
        flow.resume(&mut builder, None, 0);
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().store(MemFlagsData::trusted(), zero, registers, offset(1));
        builder.ins().jump(header, &[]);
        builder.switch_to_block(header);
        let count = builder.ins().load(types::I64, MemFlagsData::trusted(), registers, offset(1));
        let keep_running = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, count, 5);
        builder.ins().brif(keep_running, body, &[], done, &[]);
        builder.switch_to_block(body);
        let value = builder.ins().load(types::I64, MemFlagsData::trusted(), registers, 0);
        let next = builder.ins().iadd_imm_s(value, 1);
        builder.ins().store(MemFlagsData::trusted(), next, registers, 0);
        let next_count = builder.ins().iadd_imm_s(count, 1);
        builder.ins().store(MemFlagsData::trusted(), next_count, registers, offset(1));
        builder.ins().jump(header, &[]);
        builder.switch_to_block(done);
        let value = builder.ins().load(types::I64, MemFlagsData::trusted(), registers, 0);
        builder.ins().return_(&[value]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        flow.promote(&mut native, callout, None, isa.frontend_config());
        let mut stores = 0;
        let mut loads = 0;
        for block in native.layout.blocks() {
            let mut wrote = false;
            for instruction in native.layout.block_insts(block) {
                match native.dfg.insts[instruction].opcode() {
                    ir::Opcode::Store => { stores += 1; wrote = true; }
                    ir::Opcode::Load => loads += 1,
                    _ => {},
                }
            }
            if wrote {
                assert_eq!(native.dfg.insts[native.layout.last_inst(block).unwrap()].opcode(), ir::Opcode::Return,
                    "canonical homes are committed only at the recovery edge");
            }
        }
        assert_eq!(stores, 2);
        assert_eq!(loads, 2, "entry reads only; the loop carries SSA values");
        let code = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: the generated entry takes a live two-cell frame; code stays mapped.
        let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(*mut u64) -> u64>(code.pointer()) };
        for input in [0_u64, 1, 42, u64::MAX] {
            let mut frame = [input, 0];
            assert_eq!(unsafe { run(frame.as_mut_ptr()) }, input.wrapping_add(5));
            assert_eq!(frame, [input.wrapping_add(5), 5]);
        }
    }

    #[test]
    fn semantic_call_commits_then_resumes_from_changed_canonical_inputs() {
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 2]);
        signature.returns.push(AbiParam::new(types::I64));
        let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut native, &mut context);
        let entry = builder.create_block();
        let resume = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let parameters = builder.block_params(entry).to_vec();
        let registers = parameters[0];
        let result = parameters[1];
        let callout = builder.ins().iconst(types::I64, mutate_frame as *const () as usize as i64);
        let mut flow = FrameFlow::new(registers, 2);
        flow.resume(&mut builder, None, 0);
        let input = builder.ins().load(types::I64, MemFlagsData::trusted(), registers, 0);
        let incremented = builder.ins().iadd_imm_s(input, 1);
        builder.ins().store(MemFlagsData::trusted(), incremented, registers, offset(1));
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 4]);
        signature.returns.push(AbiParam::new(types::I64));
        let signature = builder.import_signature(signature);
        let pc = builder.ins().iconst(types::I64, 0);
        builder.ins().call_indirect(signature, callout, &[registers, pc, result, result]);
        builder.ins().jump(resume, &[]);
        builder.switch_to_block(resume);
        flow.resume(&mut builder, None, 0);
        let left = builder.ins().load(types::I64, MemFlagsData::trusted(), registers, 0);
        let right = builder.ins().load(types::I64, MemFlagsData::trusted(), registers, offset(1));
        let sum = builder.ins().iadd(left, right);
        builder.ins().return_(&[sum]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        flow.promote(&mut native, callout, None, isa.frontend_config());
        let code = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: both buffers match the synthetic entry and effect contract.
        let run = unsafe { std::mem::transmute::<*mut u8,
            unsafe extern "C" fn(*mut u64, *mut u64) -> u64>(code.pointer()) };
        let mut frame = [41, 0];
        let mut result = 0;
        assert_eq!(unsafe { run(frame.as_mut_ptr(), &mut result) }, 142);
        assert_eq!(frame, [100, 42]);
    }

    #[test]
    fn overlapping_frame_views_decline_promotion_before_mutation() {
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        for derived in [false, true] {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.push(AbiParam::new(types::I64));
            signature.returns.push(AbiParam::new(types::I64));
            let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut native, &mut context);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let registers = builder.block_params(entry)[0];
            let callout = builder.ins().iconst(types::I64, 0);
            let mut flow = FrameFlow::new(registers, 1);
            flow.resume(&mut builder, None, 0);
            let input = 0x1020_3040_5060_7080_u64;
            let value = builder.ins().iconst(types::I64, input as i64);
            builder.ins().store(MemFlagsData::trusted(), value, registers, 0);
            let address = if derived { builder.ins().iadd_imm_s(registers, 1) } else { registers };
            let byte = builder.ins().iconst(types::I8, 0x55);
            builder.ins().store(MemFlagsData::trusted(), byte, address, 0);
            let value = builder.ins().load(types::I64, MemFlagsData::trusted(), registers, 0);
            builder.ins().return_(&[value]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            flow.promote(&mut native, callout, None, isa.frontend_config());
            let code = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
            // SAFETY: both the whole-cell and partial accesses stay inside the live cell.
            let run = unsafe { std::mem::transmute::<*mut u8,
                unsafe extern "C" fn(*mut u64) -> u64>(code.pointer()) };
            let mut expected = input.to_ne_bytes();
            expected[usize::from(derived)] = 0x55;
            let mut frame = 0;
            assert_eq!(unsafe { run(&mut frame) }, u64::from_ne_bytes(expected));
            assert_eq!(frame, u64::from_ne_bytes(expected));
        }
    }
}
