//! Canonical frame homes are recovery edges, not the native register file.
//!
//! Resume entries admit canonical tagged inputs once. Native definitions and
//! CFG joins carry the explicit generic JavaScript type; semantic operations
//! retain their narrower Number checks. Only an effect/recovery boundary
//! materializes the live state. Guest callouts resume through fresh entries,
//! so aliased parameters and references moved by GC are never cached across one.

use super::*;
use cranelift_codegen::entity::SecondaryMap;
use cranelift_codegen::ir::InstBuilderBase;

pub(super) struct FrameFlow {
    registers: ir::Value,
    count: u16,
    effects: SecondaryMap<ir::Inst, FrameEffect>,
    footprints: Vec<RegisterSet>,
}

/// One packed column owns instruction effects. The high bits classify resume
/// roots and verified no-frame calls; other nonzero values index footprints.
/// A zero entry is an unknown effect and retains the canonical recovery path.
#[derive(Clone, Copy, Default)]
struct FrameEffect(u32);

impl FrameEffect {
    const ROOT: u32 = 1 << (u32::BITS - 1);
    const NO_FRAME: Self = Self(1 << (u32::BITS - 2));
    const INDEX_MASK: u32 = Self::NO_FRAME.0 - 1;

    fn root(register: u16) -> Self { Self(Self::ROOT | u32::from(register)) }

    #[cfg(feature = "aot")]
    fn canonical(index: usize) -> Self {
        let encoded = u32::try_from(index).ok().and_then(|index| index.checked_add(1))
            .filter(|index| *index <= Self::INDEX_MASK).expect("frame footprint index");
        Self(encoded)
    }

    fn register(self) -> Option<u16> {
        (self.0 & Self::ROOT != 0).then_some((self.0 & Self::INDEX_MASK) as u16)
    }

    fn footprint(self) -> Option<usize> {
        (self.0 != 0 && self.0 <= Self::INDEX_MASK).then(|| (self.0 - 1) as usize)
    }

    fn no_frame(self) -> bool { self.0 == Self::NO_FRAME.0 }
    fn known_call(self) -> bool { self.0 != 0 && self.0 & Self::ROOT == 0 }
}

/// Membership of a frame's registers, one bit each (`crate::bitset::BitSet`).
#[derive(Clone)]
struct RegisterSet(crate::bitset::BitSet);

impl RegisterSet {
    fn new(count: u16) -> Self { Self(crate::bitset::BitSet::new(usize::from(count))) }

    fn clear(&mut self) { self.0.clear(); }

    fn insert(&mut self, register: u16) { self.0.insert(usize::from(register)); }

    fn contains(&self, register: u16) -> bool { self.0.contains(usize::from(register)) }

    fn subtract(&mut self, other: &Self) { self.0.subtract(&other.0); }

    fn union(&mut self, other: &Self) -> bool { self.0.union_with(&other.0) }

    fn iter(&self) -> impl Iterator<Item = u16> + '_ { self.0.iter().map(|register| register as u16) }
}

impl FrameFlow {
    pub(super) fn new(registers: ir::Value, count: u16) -> Self {
        Self { registers, count, effects: SecondaryMap::new(), footprints: Vec::new() }
    }

    /// A verified canonical frame footprint, read or written without GC.
    #[cfg(feature = "aot")]
    pub(super) fn residual(&mut self, call: ir::Inst, frame: &[Register]) {
        let mut footprint = RegisterSet::new(self.count);
        for register in frame { footprint.insert(*register); }
        self.effects[call] = FrameEffect::canonical(self.footprints.len());
        self.footprints.push(footprint);
    }

    /// A helper copied from the verified typed residual: no frame access or GC.
    #[cfg(feature = "aot")]
    pub(super) fn no_frame(&mut self, call: ir::Inst) { self.effects[call] = FrameEffect::NO_FRAME; }

    pub(super) fn resume(&mut self, builder: &mut FunctionBuilder<'_>,
        live: Option<&crate::compile::liveness::RegisterMasks>, pc: usize) {
        for register in 0..self.count {
            let value = if live.is_none_or(|live| live.is_live(pc, register)) {
                builder.ins().load(types::I64, MemFlagsData::trusted(), self.registers, offset(register))
            } else { builder.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64) };
            let ir::ValueDef::Result(instruction, _) = builder.func.dfg.value_def(value) else { unreachable!() };
            self.effects[instruction] = FrameEffect::root(register);
        }
    }

    pub(super) fn promote(self, native: &mut ir::Function, callout: ir::Value,
        live: Option<&crate::compile::liveness::RegisterMasks>, config: cranelift_codegen::isa::TargetFrontendConfig) {
        let mut instructions = Vec::new();
        let records: Vec<_> = native.layout.blocks().map(|block| {
            let start = instructions.len();
            instructions.extend(native.layout.block_insts(block));
            (block, start..instructions.len())
        }).collect();
        // Reject unknown effects before changing the graph. Registered scalar
        // residuals cannot move frame buffers.
        // Other address roots belong to disjoint locals/result/VM heap storage
        // under the native-entry ABI. The canonical register pointer must not
        // escape into another view or be written as a value.
        for (_, range) in &records {
            for instruction in &instructions[range.clone()] {
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
        let mut dirty = SecondaryMap::<ir::Block, RegisterSet>::with_default(RegisterSet::new(self.count));
        loop {
            let mut changed = false;
            for (block, range) in &records {
                let mut current = dirty[*block].clone();
                let block_instructions = &instructions[range.clone()];
                for instruction in block_instructions {
                    if self.effects[*instruction].register().is_some() { current.clear(); }
                    if let Some((register, _, store)) = self.cell(native, *instruction)
                        && store { current.insert(register); }
                    if native.dfg.insts[*instruction].opcode().is_call() {
                        let effect = self.effects[*instruction];
                        if let Some(frame) = effect.footprint() {
                            current.subtract(&self.footprints[frame]);
                        } else if !effect.no_frame() { current.clear(); }
                    }
                }
                let Some(last) = block_instructions.last() else { continue };
                for destination in native.dfg.insts[*last].branch_destination(&native.dfg.jump_tables, &native.dfg.exception_tables) {
                    let incoming = &mut dirty[destination.block(&native.dfg.value_lists)];
                    changed |= incoming.union(&current);
                }
            }
            if !changed { break; }
        }
        let mut replacements = (!native.effect_specializations.is_empty() || !native.private_stores.is_empty())
            .then(|| SecondaryMap::<ir::Inst, Option<ir::Inst>>::with_capacity(native.dfg.num_insts()));
        let parameters: Vec<_> = records.iter().map(|(block, _)| native.dfg.block_params(*block).to_vec()).collect();
        for (block, _) in &records { native.layout.remove_block_and_insts(*block); }
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(native, &mut context);
        let mut blocks = SecondaryMap::<ir::Block, Option<ir::Block>>::new();
        for (block, _) in &records {
            let replacement = builder.create_block();
            if builder.func.layout.is_cold(*block) { builder.set_cold_block(replacement); }
            blocks[*block] = Some(replacement);
        }
        for ((block, _), parameters) in records.iter().zip(&parameters) {
            builder.func.dfg.detach_block_params(*block);
            for parameter in parameters {
                let next = builder.append_block_param(blocks[*block].expect("recorded native block"), builder.func.dfg.value_type(*parameter));
                builder.func.dfg.change_to_alias(*parameter, next);
            }
        }
        // All frame SSA values have semantic ANY/Tagged. Narrower types are
        // established by the shared operation's guards, never by these homes.
        let fact = typed_ir::Fact::tagged(typed_ir::BindingTime::Dynamic, typed_ir::Proof::Definition)
            .verified().expect("canonical frame definition");
        assert_eq!(fact.representation, typed_ir::Representation::Tagged);
        let variables: Vec<_> = (0..self.count).map(|_| builder.declare_var(types::I64)).collect();
        for (position, (block, range)) in records.iter().enumerate() {
            builder.switch_to_block(blocks[*block].expect("recorded native block"));
            if position == 0 {
                let undefined = builder.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
                for variable in &variables { builder.def_var(*variable, undefined); }
            }
            let mut current = dirty[*block].clone();
            for instruction in &instructions[range.clone()] {
                let data = builder.func.dfg.insts[*instruction];
                let results = builder.func.dfg.inst_results(*instruction).to_vec();
                let control = builder.func.dfg.ctrl_typevar(*instruction);
                let cell = self.cell(builder.func, *instruction);
                let effect = self.effects[*instruction];
                let root = effect.register();
                let pc = self.recovery_pc(builder.func, *instruction, callout);
                let residual = effect.footprint().map(|frame| &self.footprints[frame]);
                let no_frame = effect.no_frame();
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
                if (is_call && !no_frame) || is_return {
                    for register in current.iter() {
                        if residual.is_some_and(|frame| !frame.contains(register)) { continue; }
                        if pc.is_some_and(|pc| live.is_some_and(|live| !live.is_live(pc, register))) { continue; }
                        let value = builder.use_var(variables[usize::from(register)]);
                        builder.ins().store(MemFlagsData::trusted(), value, self.registers, offset(register));
                    }
                    match residual {
                        Some(frame) => current.subtract(frame),
                        None => current.clear(),
                    }
                }
                let mut data = data;
                let dfg = &mut builder.func.dfg;
                for destination in data.branch_destination_mut(&mut dfg.jump_tables, &mut dfg.exception_tables) {
                    if let Some(block) = blocks[destination.block(&dfg.value_lists)] {
                        destination.set_block(block, &mut dfg.value_lists);
                    }
                }
                let (next, _) = builder.ins().build(data, control);
                if let Some(replacements) = &mut replacements { replacements[*instruction] = Some(next); }
                builder.func.srclocs[next] = builder.func.srclocs[*instruction];
                builder.func.debug_tags.clone_tags(*instruction, next);
                builder.func.debug_tags.set(*instruction, []);
                builder.func.dfg.clear_results(next);
                let input = results.first().copied();
                builder.func.dfg.make_inst_results_reusing(next, control, results.into_iter().map(Some));
                if let Some(register) = root {
                    builder.def_var(variables[usize::from(register)], input.expect("resume input"));
                }
                if let Some(frame) = residual {
                    // Canonical consumers may update homes, but cannot move
                    // or release this caller's buffers.
                    for register in frame.iter() {
                        let value = builder.ins().load(types::I64, MemFlagsData::trusted(), self.registers, offset(register));
                        builder.def_var(variables[usize::from(register)], value);
                    }
                }
            }
        }
        if let Some(replacements) = replacements {
            builder.func.private_stores = std::mem::take(&mut builder.func.private_stores)
                .into_iter().filter_map(|inst| replacements[inst]).collect();
            builder.func.effect_specializations = std::mem::take(&mut builder.func.effect_specializations)
                .into_iter().filter_map(|effect| effect.remap_instructions(|inst| replacements[inst])).collect();
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
            || self.effects[instruction].known_call()
    }
}

fn offset(register: u16) -> i32 { i32::from(register) * std::mem::size_of::<Value>() as i32 }

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "aot")]
    #[test]
    fn promotion_preserves_effect_proofs_and_instruction_metadata() {
        use std::sync::atomic::AtomicU8;
        extern "C" fn owner_query() -> u64 { 1 }
        const SOURCE_OFFSET: u32 = 42;
        const OWNER_TAG: u32 = 7;
        const OWNER_LABEL: u32 = 0;
        let isa = super::super::super::jit_backend::target_isa().unwrap();
        for (tagged, conservative) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 3]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
            native.collect_debug_info();
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut native, &mut context);
            let entry = builder.create_block();
            let header = builder.create_block();
            let gate_block = builder.create_block();
            let polled = builder.create_block();
            let latch = builder.create_block();
            let exit = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.append_block_param(header, types::I64);
            builder.switch_to_block(entry);
            let args = builder.block_params(entry).to_vec();
            let callout = builder.ins().iconst(types::I64, 0);
            let mut flow = FrameFlow::new(args[0], 1);
            flow.resume(&mut builder, None, 0);
            let address = builder.ins().iconst(types::I64, owner_query as *const () as usize as i64);
            let mut query_signature = Signature::new(isa.default_call_conv());
            query_signature.returns.push(AbiParam::new(types::I64));
            let query_signature = builder.import_signature(query_signature);
            builder.set_srcloc(ir::SourceLoc::new(SOURCE_OFFSET));
            let proof = builder.ins().call_indirect(query_signature, address, &[]);
            let owner = builder.inst_results(proof)[0];
            if tagged { builder.func.debug_tags.set(proof, [ir::DebugTag::User(OWNER_TAG)]); }
            flow.no_frame(proof);
            builder.ins().jump(header, &[args[2].into()]);
            builder.switch_to_block(header);
            let count = builder.block_params(header)[0];
            // Debug values deliberately select the existing conservative poll
            // fallback, whose proof and gate must survive frame reconstruction.
            if conservative { builder.set_val_label(owner, ir::ValueLabel::from_u32(OWNER_LABEL)); }
            builder.ins().brif(count, gate_block, &[], exit, &[]);
            builder.switch_to_block(gate_block);
            let gate = builder.ins().brif(owner, latch, &[], polled, &[]);
            builder.switch_to_block(polled);
            builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), args[1]);
            builder.ins().jump(latch, &[]);
            builder.switch_to_block(latch);
            let value = builder.ins().load(types::I64, MemFlagsData::trusted(), args[0], 0);
            let next = builder.ins().iadd_imm_u(value, 1);
            builder.ins().store(MemFlagsData::trusted(), next, args[0], 0);
            let remaining = builder.ins().iadd_imm_s(count, -1);
            builder.ins().jump(header, &[remaining.into()]);
            builder.switch_to_block(exit);
            let result = builder.ins().load(types::I64, MemFlagsData::trusted(), args[0], 0);
            builder.ins().return_(&[result]);
            builder.func.effect_specializations.push(ir::function::EffectSpecialization { proof, gates: vec![(gate, false)] });
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            let location = native.srcloc(proof);
            flow.promote(&mut native, callout, None, isa.frontend_config());
            let effect = &native.effect_specializations[0];
            assert_ne!(effect.proof, proof);
            assert!(native.layout.inst_block(effect.proof).is_some());
            assert_eq!(native.dfg.insts[effect.proof].opcode(), ir::Opcode::CallIndirect);
            assert_eq!(native.srcloc(effect.proof), location);
            assert_eq!(native.debug_tags.has(effect.proof), tagged);
            assert!(!native.debug_tags.has(proof));
            assert_eq!(effect.gates.len(), 1);
            assert_ne!(effect.gates[0].0, gate);
            assert!(!effect.gates[0].1);
            assert!(native.layout.inst_block(effect.gates[0].0).is_some());
            assert_eq!(native.dfg.insts[effect.gates[0].0].opcode(), ir::Opcode::Brif);
            let mut compiler = cranelift_codegen::Context::for_function(native);
            let compiled = compiler.compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default()).unwrap();
            let memory = CodeMemory::new(compiled.buffer.data()).unwrap();
            let mut calls = 0;
            let mut polls = 0;
            for block in compiler.func.layout.blocks() { for inst in compiler.func.layout.block_insts(block) {
                calls += usize::from(compiler.func.dfg.insts[inst].opcode().is_call());
                polls += usize::from(compiler.func.dfg.insts[inst].opcode() == ir::Opcode::AtomicLoad);
            }}
            assert_eq!(calls, usize::from(tagged || !conservative),
                "only an observable tag or unswitched loop retains the query: {}", compiler.func);
            assert_eq!(polls, 1, "the conservative fallback must keep cancellation polling");
            assert!(compiler.func.effect_specializations.is_empty());
            // SAFETY: the verified ABI receives a live word, an atomic byte,
            // and a bounded trip count. The code mapping remains live.
            let run = unsafe { std::mem::transmute::<*mut u8,
                unsafe extern "C" fn(*mut u64, *const AtomicU8, u64) -> u64>(memory.pointer()) };
            for trips in [0, 1, 17, 257] {
                let mut frame = 10;
                let poll = AtomicU8::new(0);
                assert_eq!(unsafe { run(&mut frame, &poll, trips) }, 10 + trips);
                assert_eq!(frame, 10 + trips);
            }
        }
    }

    #[test]
    fn packed_effects_preserve_zero_roots_and_distinguish_unknown_calls() {
        assert_eq!(std::mem::size_of::<FrameEffect>(), std::mem::size_of::<u32>());
        assert!(!FrameEffect::default().known_call());
        assert_eq!(FrameEffect::default().register(), None);
        assert_eq!(FrameEffect::default().footprint(), None);
        for register in [0, u16::MAX] {
            let effect = FrameEffect::root(register);
            assert_eq!(effect.register(), Some(register));
            assert!(!effect.known_call());
            assert_eq!(effect.footprint(), None);
        }
        assert!(FrameEffect::NO_FRAME.known_call());
        assert!(FrameEffect::NO_FRAME.no_frame());
        assert_eq!(FrameEffect::NO_FRAME.footprint(), None);
        #[cfg(feature = "aot")]
        for index in [0, FrameEffect::INDEX_MASK as usize - 1] {
            let effect = FrameEffect::canonical(index);
            assert_eq!(effect.footprint(), Some(index));
            assert!(effect.known_call());
            assert!(!effect.no_frame());
            assert_eq!(effect.register(), None);
        }
    }

    #[test]
    fn register_bitsets_batch_joins_and_kills_across_inline_and_spilled_words() {
        const WORD_BITS: usize = u64::BITS as usize;
        const INLINE_WORDS: usize = 2;
        for count in [0, 1, WORD_BITS as u16, (WORD_BITS * INLINE_WORDS + 1) as u16, u16::MAX] {
            let mut left = RegisterSet::new(count);
            let mut right = RegisterSet::new(count);
            for register in 0..count {
                if register % 2 == 0 { left.insert(register); } else { right.insert(register); }
            }
            assert_eq!(left.0.spilled(), usize::from(count) > WORD_BITS * INLINE_WORDS);
            assert_eq!(left.union(&right), count > 1);
            assert!(!left.union(&right));
            assert_eq!(left.iter().collect::<Vec<_>>(), (0..count).collect::<Vec<_>>());
            left.subtract(&right);
            assert_eq!(left.iter().collect::<Vec<_>>(), (0..count).filter(|register| register % 2 == 0).collect::<Vec<_>>());
            for register in 0..count { assert_eq!(left.contains(register), register % 2 == 0); }
            left.clear();
            assert_eq!(left.iter().next(), None);
        }
    }

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
        let isa = super::super::super::jit_backend::target_isa().unwrap();
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
        let isa = super::super::super::jit_backend::target_isa().unwrap();
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
        builder.set_cold_block(done);
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
        let cold: Vec<_> = native.layout.blocks().filter(|block| native.layout.is_cold(*block)).collect();
        assert_eq!(cold.len(), 1, "SSA promotion must preserve the recovery path's cold placement");
        assert_eq!(native.dfg.insts[native.layout.last_inst(cold[0]).unwrap()].opcode(), ir::Opcode::Return);
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
        let isa = super::super::super::jit_backend::target_isa().unwrap();
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
        let isa = super::super::super::jit_backend::target_isa().unwrap();
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
