//! The lane region specializer: Futamura's first projection of the interpreter onto one
//! JavaScript function, run on the function's frameless lane activations under Ershov's
//! online division (docs/v2/specialization.md).
//!
//! What the division makes static is folded, and each dynamic fact is proved once:
//!
//! - Every bytecode register is an SSA value in machine registers. The register file is
//!   read at entry and written back only where the region leaves.
//! - A Number keeps an unboxed `i32` view beside its tagged bits. An instruction proves a
//!   register int32 once; later instructions in the block use the view, and the tag is
//!   rebuilt only where a tagged value is needed.
//! - Region-wide facts are proved at entry: the receiver's record, shape and storage, the
//!   shapes the receiver sites fold, the method-cache state stores need, and capture reads.
//! - Field sites fold their cache entries (shape and slot) and constants fold to immediates.
//!
//! The region leaves at the exact PC of any instruction it does not run, before any of
//! that instruction's effects, with the register file holding the interpreter's state; the
//! lane resumes there. Nothing in a run allocates, collects or calls guest code.

use super::*;
use crate::bytecode::FieldLayout;
use std::collections::{BTreeMap, BTreeSet};

/// What the specializer knows of a register within a basic block.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Known {
    /// Only its tagged bits are current.
    Tagged,
    /// An int32 Number: the unboxed view is current; the tagged bits are too when `boxed`.
    Int { boxed: bool },
}

struct Registers {
    tagged: Vec<Variable>,
    int: Vec<Variable>,
}

/// The compile-time state of the instruction being lowered.
#[derive(Clone)]
struct State {
    known: Vec<Known>,
    /// Registers proved to hold a direct object, this block: its record address and shape.
    shapes: BTreeMap<Register, (ir::Value, ir::Value)>,
    /// Registers holding the receiver, whose facts the entry proved.
    receivers: BTreeSet<Register>,
}

impl State {
    fn canonical(registers: usize) -> Self {
        Self { known: vec![Known::Tagged; registers], shapes: BTreeMap::new(), receivers: BTreeSet::new() }
    }
}

/// Region-wide facts proved at entry.
struct Entry {
    receiver: LaneReceiver,
    /// Per folded receiver shape, whether the receiver has it.
    receiver_shapes: BTreeMap<u32, Variable>,
    /// Per stored atom, whether the run admits direct stores of it.
    store_atoms: BTreeMap<Atom, Variable>,
    /// Per capture slot read, its value or `DELETED`.
    captures: BTreeMap<u16, Variable>,
    this: Variable,
}

pub(super) struct LaneRegion {
    pub(super) code: Vec<u8>,
    /// Instructions the region runs natively, for admission.
    pub(super) native: usize,
}

struct Lowering<'a, 'f> {
    builder: FunctionBuilder<'f>,
    program: &'a ResidualProgram,
    function: &'a Function,
    helpers: LaneHelpers,
    fields: &'a [Vec<ReceiverField>],
    context: ir::Value,
    payload: ir::Value,
    registers_base: ir::Value,
    result: ir::Value,
    registers: Registers,
    written: BTreeSet<Register>,
    blocks: Vec<Option<ir::Block>>,
    exits: BTreeMap<u32, ir::Block>,
    entry: Option<Entry>,
    native: usize,
    /// Guard failure blocks to fill once the region is lowered (`guard_exit`).
    pending: Vec<(ir::Block, State, u32)>,
}

fn register_offset(register: Register) -> i32 {
    i32::from(register) * std::mem::size_of::<u64>() as i32
}

fn int_tagged(builder: &mut FunctionBuilder<'_>, int: ir::Value) -> ir::Value {
    let payload = builder.ins().uextend(types::I64, int);
    builder.ins().bor_imm_u(payload, crate::value::ValueLayout::INT_TAG as i64)
}

fn is_int_tagged(builder: &mut FunctionBuilder<'_>, bits: ir::Value) -> ir::Value {
    let shift = i64::from(crate::value::ValueLayout::TAG_SHIFT);
    let tag = builder.ins().ushr_imm_u(bits, shift);
    builder.ins().icmp_imm_u(IntCC::Equal, tag, (crate::value::ValueLayout::INT_TAG >> shift) as i64)
}

/// The registers an instruction writes, by its operand layouts.
fn writes(instruction: WideInstruction) -> Vec<Register> {
    let mut written = Vec::new();
    for field in crate::bytecode::InstructionField::ALL {
        let layout = instruction.op().field_layout(*field);
        let value = match field {
            crate::bytecode::InstructionField::A => instruction.a(),
            crate::bytecode::InstructionField::B => instruction.b(),
            crate::bytecode::InstructionField::C => instruction.c(),
        };
        match layout {
            FieldLayout::ResultRegister | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister => {
                written.push(value & crate::bytecode::REGISTER_MASK)
            }
            _ => {}
        }
    }
    written
}

impl<'a, 'f> Lowering<'a, 'f> {
    fn ins(&mut self) -> cranelift_frontend::FuncInstBuilder<'_, 'f> {
        self.builder.ins()
    }

    fn tagged(&mut self, state: &mut State, register: Register) -> ir::Value {
        let index = usize::from(register);
        if let Known::Int { boxed: false } = state.known[index] {
            let int = self.builder.use_var(self.registers.int[index]);
            let bits = int_tagged(&mut self.builder, int);
            self.builder.def_var(self.registers.tagged[index], bits);
            state.known[index] = Known::Int { boxed: true };
        }
        self.builder.use_var(self.registers.tagged[index])
    }

    fn set_tagged(&mut self, state: &mut State, register: Register, bits: ir::Value) {
        let index = usize::from(register);
        self.builder.def_var(self.registers.tagged[index], bits);
        state.known[index] = Known::Tagged;
        state.shapes.remove(&register);
        state.receivers.remove(&register);
    }

    fn set_int(&mut self, state: &mut State, register: Register, int: ir::Value) {
        let index = usize::from(register);
        self.builder.def_var(self.registers.int[index], int);
        state.known[index] = Known::Int { boxed: false };
        state.shapes.remove(&register);
        state.receivers.remove(&register);
    }

    /// The register's int32 view, proving it from the tag once; `None` where it is not
    /// int32 at run time, taking `otherwise`.
    fn int(&mut self, state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
        let index = usize::from(register);
        if let Known::Int { .. } = state.known[index] {
            return self.builder.use_var(self.registers.int[index]);
        }
        let bits = self.builder.use_var(self.registers.tagged[index]);
        let integer = is_int_tagged(&mut self.builder, bits);
        emit_js_guard(&mut self.builder, integer, otherwise);
        let int = self.ins().ireduce(types::I32, bits);
        self.builder.def_var(self.registers.int[index], int);
        state.known[index] = Known::Int { boxed: true };
        int
    }

    /// Box every unboxed register, as a block boundary or an exit needs.
    fn canonicalize(&mut self, state: &mut State) {
        for register in 0..state.known.len() {
            if let Known::Int { boxed: false } = state.known[register] {
                self.tagged(state, register as Register);
            }
        }
    }

    /// Leave at `pc` with the interpreter's state: every written register back in the file.
    fn exit(&mut self, state: &mut State, pc: u32) {
        self.canonicalize(state);
        let block = match self.exits.get(&pc) {
            Some(block) => *block,
            None => {
                let block = self.builder.create_block();
                self.builder.set_cold_block(block);
                self.exits.insert(pc, block);
                block
            }
        };
        self.ins().jump(block, &[]);
    }

    /// A guard's failure block: it leaves at `pc` from the state where the guard sits. It is
    /// filled after the region, since the current block is still open.
    fn guard_exit(&mut self, state: &State, pc: u32) -> ir::Block {
        let block = self.builder.create_block();
        self.builder.set_cold_block(block);
        self.pending.push((block, state.clone(), pc));
        block
    }

    fn continue_at(&mut self, state: &mut State, pc: usize) {
        self.canonicalize(state);
        match self.blocks.get(pc).copied().flatten() {
            Some(block) => {
                self.ins().jump(block, &[]);
            }
            None => self.exit(state, pc as u32),
        }
    }

    fn operand_tagged(&mut self, state: &mut State, operand: Operand) -> Option<ir::Value> {
        match operand.kind()? {
            OperandKind::Register => Some(self.tagged(state, operand.register_index()?)),
            OperandKind::Constant => {
                let constant = self.program.constants.get(usize::from(operand.payload()))?;
                let value = constant_value(constant)?;
                Some(self.ins().iconst(types::I64, value.to_bits() as i64))
            }
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    /// An operand's int32 value, statically where it is a constant int32.
    fn operand_int(&mut self, state: &mut State, operand: Operand, otherwise: ir::Block) -> Option<ir::Value> {
        match operand.kind()? {
            OperandKind::Register => Some(self.int(state, operand.register_index()?, otherwise)),
            OperandKind::Constant => {
                let constant = self.program.constants.get(usize::from(operand.payload()))?;
                let value = constant_value(constant)?;
                if is_int32_value(value) {
                    Some(self.ins().iconst(types::I32, i64::from(value.wasm_bits32() as i32)))
                } else {
                    self.ins().jump(otherwise, &[]);
                    let dead = self.builder.create_block();
                    self.builder.switch_to_block(dead);
                    Some(self.ins().iconst(types::I32, 0))
                }
            }
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    fn operand_supported(&self, operand: Operand) -> bool {
        match operand.kind() {
            Some(OperandKind::Register) => operand.register_index().is_some_and(|r| r < self.function.registers),
            Some(OperandKind::Constant) => self
                .program
                .constants
                .get(usize::from(operand.payload()))
                .and_then(constant_value)
                .is_some(),
            _ => false,
        }
    }
}

/// A constant's value where it is an immediate (no heap allocation): numbers, booleans,
/// `null` and `undefined`.
/// Whether an immediate value is an int32 Number in its tagged encoding.
fn is_int32_value(value: Value) -> bool {
    let shift = crate::value::ValueLayout::TAG_SHIFT;
    value.to_bits() >> shift == crate::value::ValueLayout::INT_TAG >> shift
}

fn constant_value(constant: &crate::bytecode::Constant) -> Option<Value> {
    use crate::bytecode::Constant;
    match constant {
        Constant::Number(number) => Some(Value::number(*number)),
        Constant::Boolean(value) => Some(Value::boolean(*value)),
        Constant::Null => Some(Value::NULL),
        Constant::Undefined => Some(Value::UNDEFINED),
        _ => None,
    }
}

/// The PCs that begin basic blocks: the entry, every jump target and every instruction
/// after a jump.
fn leaders(function: &Function) -> Vec<bool> {
    let mut leaders = vec![false; function.code.len()];
    if let Some(first) = leaders.first_mut() {
        *first = true;
    }
    for (pc, packed) in function.code.iter().enumerate() {
        let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
        if matches!(instruction.op().control_flow_layout(), ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump) {
            if let Some(target) = usize::try_from(instruction.jump_target()).ok().filter(|t| *t < leaders.len()) {
                leaders[target] = true;
            }
            if let Some(next) = leaders.get_mut(pc + 1) {
                *next = true;
            }
        }
    }
    leaders
}

pub(super) fn compile(
    program: &ResidualProgram,
    function_id: u32,
    helpers: LaneHelpers,
    fields: &[Vec<ReceiverField>],
) -> Option<LaneRegion> {
    let function = program.function_get(function_id as usize)?;
    if function.code.is_empty() || function.code.len() > MAX_JS_GENERAL_DISPATCH_BLOCKS {
        return None;
    }
    let isa = super::jit_backend::target_isa("speed")?;
    let signature = native_entry_signature(isa.pointer_type(), isa.default_call_conv());
    let mut native = ir::Function::with_name_signature(UserFuncName::user(0x514c_5247, function_id), signature);
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    let entry_block = builder.create_block();
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    let parameters = builder.block_params(entry_block).to_vec();
    let (context, registers_base, result) = (parameters[0], parameters[3], parameters[4]);
    let payload = builder.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        context,
        std::mem::offset_of!(CallContext, adapter_payload) as i32,
    );
    let count = usize::from(function.registers);
    let registers = Registers {
        tagged: (0..count).map(|_| builder.declare_var(types::I64)).collect(),
        int: (0..count).map(|_| builder.declare_var(types::I32)).collect(),
    };
    let zero = builder.ins().iconst(types::I32, 0);
    for register in 0..count {
        let bits = builder.ins().load(types::I64, MemFlagsData::trusted(), registers_base, register_offset(register as Register));
        builder.def_var(registers.tagged[register], bits);
        builder.def_var(registers.int[register], zero);
    }
    let leaders = leaders(function);
    let blocks: Vec<_> = leaders.iter().map(|leader| leader.then(|| builder.create_block())).collect();
    let mut lowering = Lowering {
        builder,
        program,
        function,
        helpers,
        fields,
        context,
        payload,
        registers_base,
        result,
        registers,
        written: BTreeSet::new(),
        blocks,
        exits: BTreeMap::new(),
        entry: None,
        native: 0,
        pending: Vec::new(),
    };
    let instructions: Vec<_> = function
        .code
        .iter()
        .map(|packed| super::dispatch_fast::decoded_instruction(function, *packed))
        .collect();
    for instruction in &instructions {
        lowering.written.extend(writes(*instruction).into_iter().filter(|r| usize::from(*r) < count));
    }
    lowering.entry = Some(lowering.entry_facts(&instructions));
    let first = lowering.blocks[0].expect("pc 0 leads");
    lowering.builder.ins().jump(first, &[]);

    let mut state = State::canonical(count);
    let mut reachable = false;
    for (pc, instruction) in instructions.iter().copied().enumerate() {
        if let Some(block) = lowering.blocks[pc] {
            if reachable {
                lowering.canonicalize(&mut state);
                lowering.builder.ins().jump(block, &[]);
            }
            lowering.builder.switch_to_block(block);
            state = State::canonical(count);
            reachable = true;
        }
        if !reachable {
            continue;
        }
        reachable = lowering.instruction(&mut state, pc, instruction, &instructions);
    }
    if reachable {
        lowering.exit(&mut state, instructions.len() as u32);
    }
    for (block, mut leaving, pc) in std::mem::take(&mut lowering.pending) {
        lowering.builder.switch_to_block(block);
        lowering.exit(&mut leaving, pc);
    }
    // The exit blocks: write back every register the function writes, then leave at the PC.
    let exits: Vec<_> = lowering.exits.iter().map(|(pc, block)| (*pc, *block)).collect();
    for (pc, block) in exits {
        lowering.builder.switch_to_block(block);
        for register in lowering.written.clone() {
            let bits = lowering.builder.use_var(lowering.registers.tagged[usize::from(register)]);
            lowering.builder.ins().store(MemFlagsData::trusted(), bits, lowering.registers_base, register_offset(register));
        }
        let pc_value = lowering.builder.ins().iconst(types::I64, i64::from(pc));
        lowering.builder.ins().store(MemFlagsData::trusted(), pc_value, lowering.result, 0);
        let status = lowering.builder.ins().iconst(types::I32, i64::from(NATIVE_JS_STOPPED));
        lowering.builder.ins().return_(&[status]);
    }
    let native_count = lowering.native;
    let _ = lowering.context;
    lowering.builder.seal_all_blocks();
    lowering.builder.finalize(isa.frontend_config());
    let code = compile_native_function(native, &*isa, super::js_compiled::MAX_CODE_BYTES, |code| Some(code.to_vec())).ok()?;
    Some(LaneRegion { code, native: native_count })
}

impl Lowering<'_, '_> {
    fn entry_facts(&mut self, instructions: &[WideInstruction]) -> Entry {
        let receiver_offset = i32::try_from(std::mem::offset_of!(JsStepDispatch, receiver)).expect("dispatch offset fits i32");
        let receiver = emit_lane_receiver(&mut self.builder, self.payload, receiver_offset);
        let this_bits = self.builder.ins().load(types::I64, MemFlagsData::trusted(), self.payload, receiver_offset);
        let this = self.builder.declare_var(types::I64);
        self.builder.def_var(this, this_bits);
        let mut receiver_shapes = BTreeMap::new();
        for field in self.fields.iter().flatten() {
            if receiver_shapes.contains_key(&field.shape) {
                continue;
            }
            let shape = self.builder.use_var(receiver.shape);
            let same = self.builder.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape));
            let variable = self.builder.declare_var(types::I8);
            self.builder.def_var(variable, same);
            receiver_shapes.insert(field.shape, variable);
        }
        // A direct store of an atom needs a specialized run and no method cache naming it;
        // nothing in a run changes either (`emit_js_store_admits`).
        let mut store_atoms = BTreeMap::new();
        for instruction in instructions {
            if matches!(instruction.op(), Op::SetThisField | Op::SetThisFieldStrict) && !store_atoms.contains_key(&instruction.atom_index()) {
                let atom = instruction.atom_index();
                let admitted = self.emit_store_atom_admitted(atom);
                let variable = self.builder.declare_var(types::I8);
                self.builder.def_var(variable, admitted);
                store_atoms.insert(atom, variable);
            }
        }
        let mut captures = BTreeMap::new();
        for instruction in instructions {
            if instruction.op() == Op::LoadCapture && crate::bytecode::ImmediateLayout::capture_depth(instruction.imm()) == 0 {
                let slot = crate::bytecode::ImmediateLayout::capture_slot(instruction.imm());
                if captures.contains_key(&slot) {
                    continue;
                }
                let slots = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, captures));
                let slot_value = self.builder.ins().iconst(types::I64, i64::from(slot));
                let mut signature = Signature::new(self.builder.func.signature.call_conv);
                signature.params.extend([AbiParam::new(types::I64), AbiParam::new(types::I64)]);
                signature.returns.push(AbiParam::new(types::I64));
                let signature = self.builder.import_signature(signature);
                let helper = self.builder.ins().iconst(types::I64, lane_capture_read as unsafe extern "C" fn(u64, u64) -> u64 as usize as i64);
                let call = self.builder.ins().call_indirect(signature, helper, &[slots, slot_value]);
                let value = self.builder.inst_results(call)[0];
                let variable = self.builder.declare_var(types::I64);
                self.builder.def_var(variable, value);
                captures.insert(slot, variable);
            }
        }
        Entry { receiver, receiver_shapes, store_atoms, captures, this }
    }

    fn emit_store_atom_admitted(&mut self, atom: Atom) -> ir::Value {
        let specialized = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, stores_specialized));
        let specialized = self.builder.ins().icmp_imm_u(IntCC::NotEqual, specialized, 0);
        let word_index = atom / u64::BITS;
        let words = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, method_cache_atoms));
        let words_len = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, method_cache_atom_words));
        let covered = self.builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, words_len, i64::from(word_index));
        let present = self.builder.create_block();
        let join = self.builder.create_block();
        let free = self.builder.append_block_param(join, types::I8);
        let one = self.builder.ins().iconst(types::I8, 1);
        self.builder.ins().brif(covered, present, &[], join, &[ir::BlockArg::Value(one)]);
        self.builder.switch_to_block(present);
        let word = self.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            words,
            i32::try_from(word_index as usize * std::mem::size_of::<u64>()).expect("atom word offset fits i32"),
        );
        let named = self.builder.ins().band_imm_u(word, (1_u64 << (atom % u64::BITS)) as i64);
        let unnamed = self.builder.ins().icmp_imm_u(IntCC::Equal, named, 0);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(unnamed)]);
        self.builder.switch_to_block(join);
        self.builder.ins().band(specialized, free)
    }

    /// Lower the instruction at `pc`; whether control falls through to the next.
    fn instruction(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, instructions: &[WideInstruction]) -> bool {
        let pc32 = pc as u32;
        let plain = instruction.result_flags() == 0;
        let ran = match instruction.op() {
            Op::Nop if plain => Some(true),
            Op::LoadConst if plain => {
                let value = self.program.constants.get(instruction.constant_index()).and_then(constant_value);
                match value {
                    Some(value) if is_int32_value(value) => {
                        let int = self.ins().iconst(types::I32, i64::from(value.wasm_bits32() as i32));
                        self.set_int(state, instruction.result_register(), int);
                        Some(true)
                    }
                    Some(value) => {
                        let bits = self.ins().iconst(types::I64, value.to_bits() as i64);
                        self.set_tagged(state, instruction.result_register(), bits);
                        Some(true)
                    }
                    None => None,
                }
            }
            Op::Move if plain => {
                let (destination, source) = (instruction.result_register(), instruction.register_b());
                let known = state.known[usize::from(source)];
                match known {
                    Known::Int { boxed } => {
                        let int = self.builder.use_var(self.registers.int[usize::from(source)]);
                        self.set_int(state, destination, int);
                        if boxed {
                            let bits = self.builder.use_var(self.registers.tagged[usize::from(source)]);
                            self.builder.def_var(self.registers.tagged[usize::from(destination)], bits);
                            state.known[usize::from(destination)] = Known::Int { boxed: true };
                        }
                    }
                    Known::Tagged => {
                        let bits = self.tagged(state, source);
                        self.set_tagged(state, destination, bits);
                    }
                }
                if let Some(shape) = state.shapes.get(&source).copied() {
                    state.shapes.insert(destination, shape);
                }
                if state.receivers.contains(&source) {
                    state.receivers.insert(destination);
                }
                Some(true)
            }
            Op::LoadThis if plain && !instruction.writes_current_this() => {
                let exit = self.guard_exit(state, pc32);
                let entry = self.entry.as_ref().expect("entry facts");
                let this = self.builder.use_var(entry.this);
                let bound = self.ins().icmp_imm_u(IntCC::NotEqual, this, Value::DELETED.to_bits() as i64);
                emit_js_guard(&mut self.builder, bound, exit);
                self.set_tagged(state, instruction.result_register(), this);
                state.receivers.insert(instruction.result_register());
                Some(true)
            }
            Op::LoadCapture if plain && crate::bytecode::ImmediateLayout::capture_depth(instruction.imm()) == 0 => {
                let slot = crate::bytecode::ImmediateLayout::capture_slot(instruction.imm());
                let variable = self.entry.as_ref().and_then(|entry| entry.captures.get(&slot)).copied();
                variable.map(|variable| {
                    let exit = self.guard_exit(state, pc32);
                    let value = self.builder.use_var(variable);
                    let read = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
                    emit_js_guard(&mut self.builder, read, exit);
                    self.set_tagged(state, instruction.result_register(), value);
                    true
                })
            }
            Op::GetField if plain && !instruction.writes_current_this() => self.get_field(state, pc, instruction),
            Op::SetThisField | Op::SetThisFieldStrict if plain => self.set_this_field(state, pc, instruction),
            op if op.js_denotation() == JsDenotation::NumericBinary && instruction.numeric_local_target().is_none() => {
                self.binary(state, pc, instruction, instructions)
            }
            Op::IncDec if plain && instruction.numeric_local_target().is_none() => {
                let decrement = instruction.boolean_flag();
                decrement.map(|decrement| {
                    let exit = self.guard_exit(state, pc32);
                    let value = self.int(state, instruction.register_b(), exit);
                    let (result, overflow) = if decrement {
                        let one = self.ins().iconst(types::I32, 1);
                        self.ins().ssub_overflow(value, one)
                    } else {
                        let one = self.ins().iconst(types::I32, 1);
                        self.ins().sadd_overflow(value, one)
                    };
                    emit_js_guard_not(&mut self.builder, overflow, exit);
                    self.set_int(state, instruction.result_register(), result);
                    true
                })
            }
            Op::Jump if plain => {
                let target = instruction.jump_target() as usize;
                if target <= pc {
                    self.interrupt_check(state, pc32);
                }
                self.continue_at(state, target);
                return false;
            }
            Op::JumpFalse | Op::JumpTrue if plain => {
                let take_when_truthy = instruction.op() == Op::JumpTrue;
                let exit = self.guard_exit(state, pc32);
                let register = instruction.register_a();
                let truthy = match state.known[usize::from(register)] {
                    Known::Int { .. } => {
                        let int = self.builder.use_var(self.registers.int[usize::from(register)]);
                        self.ins().icmp_imm_u(IntCC::NotEqual, int, 0)
                    }
                    Known::Tagged => {
                        let bits = self.tagged(state, register);
                        emit_js_tagged_truthiness(&mut self.builder, bits, exit)
                    }
                };
                let taken = if take_when_truthy { truthy } else { self.ins().icmp_imm_u(IntCC::Equal, truthy, 0) };
                self.branch(state, pc, instruction.jump_target() as usize, taken);
                return false;
            }
            Op::JumpBinaryFalse if plain => {
                let operator = instruction.binary_operator_field();
                let (left, right) = (instruction.operand_b(), instruction.operand_c());
                if super::operations::NumberRelation::from_operator(operator).is_none()
                    || !self.operand_supported(left)
                    || !self.operand_supported(right)
                {
                    None
                } else {
                    let exit = self.guard_exit(state, pc32);
                    let left = self.operand_int(state, left, exit).expect("supported operand");
                    let right = self.operand_int(state, right, exit).expect("supported operand");
                    let holds = emit_number_relation(&mut self.builder, operator, left, right, NumberRelationDomain::Int32)
                        .expect("a numeric relation");
                    let taken = self.ins().icmp_imm_u(IntCC::Equal, holds, 0);
                    let target = instruction.jump_target() as usize;
                    if target <= pc {
                        self.interrupt_check(state, pc32);
                    }
                    self.branch(state, pc, target, taken);
                    return false;
                }
            }
            _ => None,
        };
        match ran {
            Some(falls) => {
                self.native += 1;
                falls
            }
            None => {
                self.exit(state, pc32);
                false
            }
        }
    }

    fn interrupt_check(&mut self, state: &State, pc: u32) {
        let exit = self.guard_exit(state, pc);
        let context = self.context;
        let flag = self.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            context,
            std::mem::offset_of!(CallContext, interrupt) as i32,
        );
        let requested = self.ins().load(types::I8, MemFlagsData::trusted(), flag, 0);
        let clear = self.ins().icmp_imm_u(IntCC::Equal, requested, 0);
        emit_js_guard(&mut self.builder, clear, exit);
    }

    fn branch(&mut self, state: &mut State, pc: usize, target: usize, taken: ir::Value) {
        self.canonicalize(state);
        let taken_block = self.builder.create_block();
        let fall_block = self.builder.create_block();
        self.ins().brif(taken, taken_block, &[], fall_block, &[]);
        self.builder.switch_to_block(taken_block);
        let mut taken_state = state.clone();
        self.continue_at(&mut taken_state, target);
        self.builder.switch_to_block(fall_block);
        let mut fall_state = state.clone();
        self.continue_at(&mut fall_state, pc + 1);
    }

    fn get_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let Some(FieldLookup::Atom { base, cache_site, .. }) = instruction.field_lookup() else { return None };
        if instruction.returns_from_frame() {
            return None;
        }
        let destination = instruction.result_register();
        let exit = self.guard_exit(state, pc as u32);
        let shapes = self.fields.get(pc).cloned().unwrap_or_default();
        let register = base.register_index();
        if register.is_some_and(|register| register >= self.function.registers) {
            return None;
        }
        let receiver = base == crate::bytecode::FieldBase::THIS
            || register.is_some_and(|register| state.receivers.contains(&register));
        if !shapes.is_empty() {
            // The site's proved shapes and slots, folded: one shape load of the object (the
            // receiver's was proved at entry), a compare per shape, a direct read.
            let (object, shape, inline) = if receiver {
                let entry = self.entry.as_ref().expect("entry facts");
                (
                    self.builder.use_var(entry.receiver.object),
                    self.builder.use_var(entry.receiver.shape),
                    self.builder.use_var(entry.receiver.inline),
                )
            } else {
                self.object_record(state, register.expect("a register base"), exit)
            };
            let value = self.shape_switch_read(object, shape, inline, &shapes, exit);
            self.set_tagged(state, destination, value);
            return Some(true);
        }
        let object_bits = if receiver && register.is_none() {
            let entry = self.entry.as_ref().expect("entry facts");
            self.builder.use_var(entry.this)
        } else {
            self.tagged(state, register?)
        };
        let value = self.field_probe(object_bits, cache_site, exit);
        self.set_tagged(state, destination, value);
        Some(true)
    }

    /// Read the slot `shapes` names for the record's `shape`; `exit` for any other shape.
    fn shape_switch_read(
        &mut self,
        object: ir::Value,
        shape: ir::Value,
        inline: ir::Value,
        shapes: &[ReceiverField],
        exit: ir::Block,
    ) -> ir::Value {
        let entry = self.entry.as_ref().expect("entry facts");
        let properties = self.builder.use_var(entry.receiver.properties);
        let properties_len = self.builder.use_var(entry.receiver.properties_len);
        let join = self.builder.create_block();
        let value = self.builder.append_block_param(join, types::I64);
        for field in shapes {
            let matched = self.builder.create_block();
            let next = self.builder.create_block();
            let same = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape));
            self.ins().brif(same, matched, &[], next, &[]);
            self.builder.switch_to_block(matched);
            let read = emit_object_slot_read(&mut self.builder, object, inline, properties, properties_len, field.slot, exit);
            self.ins().jump(join, &[ir::BlockArg::Value(read)]);
            self.builder.switch_to_block(next);
        }
        self.ins().jump(exit, &[]);
        self.builder.switch_to_block(join);
        value
    }

    /// The record address, exact shape and inline-storage bits of the direct object in
    /// `register`, proved once per block (`State::shapes`).
    fn object_record(&mut self, state: &mut State, register: Register, exit: ir::Block) -> (ir::Value, ir::Value, ir::Value) {
        let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
        let (object, shape) = match state.shapes.get(&register).copied() {
            Some(known) => known,
            None => {
                let bits = self.tagged(state, register);
                let family = self.ins().band_imm_u(bits, crate::value::ValueLayout::HEAP_FAMILY_MASK as i64);
                let is_object = self.ins().icmp_imm_u(IntCC::Equal, family, crate::value::ValueLayout::DIRECT_OBJECT_TAG as i64);
                emit_js_guard(&mut self.builder, is_object, exit);
                let record_id = self.ins().ireduce(types::I32, bits);
                let in_range = self.ins().icmp_imm_u(IntCC::UnsignedLessThan, record_id, layout.record_capacity as i64);
                emit_js_guard(&mut self.builder, in_range, exit);
                let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, object_record_base));
                let present = self.ins().icmp_imm_u(IntCC::NotEqual, records, 0);
                emit_js_guard(&mut self.builder, present, exit);
                let record_id = self.ins().uextend(types::I64, record_id);
                let offset = self.ins().imul_imm_u(record_id, layout.record_size as i64);
                let object = self.ins().iadd(records, offset);
                // The exact shape carries the dictionary bit, so a dictionary never matches.
                let shape = self.ins().load(types::I32, MemFlagsData::trusted(), object, layout.auxiliary_offset as i32);
                state.shapes.insert(register, (object, shape));
                (object, shape)
            }
        };
        let meta = self.ins().load(types::I32, MemFlagsData::trusted(), object, layout.meta_offset as i32);
        let inline = self.ins().band_imm_u(meta, i64::from(layout.inline_storage_mask));
        (object, shape, inline)
    }

    /// The VM's own field-cache read (`Vm::cached_field_hit`), prototype holders included;
    /// a miss leaves through `exit`.
    fn field_probe(&mut self, object: ir::Value, site: u16, exit: ir::Block) -> ir::Value {
        let site = self.ins().iconst(types::I64, i64::from(site));
        let value = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.field_get, &[object, site]);
        let hit = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, hit, exit);
        value
    }

    fn set_this_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let source = instruction.register_a();
        if source >= self.function.registers || instruction.writes_current_this() || instruction.returns_from_frame() {
            return None;
        }
        let exit = self.guard_exit(state, pc as u32);
        let atom = instruction.atom_index();
        let known_int = matches!(state.known[usize::from(source)], Known::Int { .. });
        let value = self.tagged(state, source);
        let entry = self.entry.as_ref().expect("entry facts");
        if let Some(field) = self.fields.get(pc).and_then(|shapes| (shapes.len() == 1).then(|| shapes[0])) {
            let same = self.builder.use_var(entry.receiver_shapes[&field.shape]);
            let admitted = self.builder.use_var(entry.store_atoms[&atom]);
            let inline = self.builder.use_var(entry.receiver.inline);
            let object = self.builder.use_var(entry.receiver.object);
            let properties = self.builder.use_var(entry.receiver.properties);
            let properties_len = self.builder.use_var(entry.receiver.properties_len);
            let this = self.builder.use_var(entry.this);
            let direct = self.builder.create_block();
            let general = self.builder.create_block();
            let done = self.builder.create_block();
            let arena = self.ins().icmp_imm_u(IntCC::Equal, inline, 0);
            let ok = self.ins().band(same, admitted);
            let ok = self.ins().band(ok, arena);
            // An int32 needs no barrier; any other value is tested once here.
            let ok = if known_int {
                ok
            } else {
                let tag = self.ins().band_imm_u(value, crate::value::ValueLayout::TAG_MASK as i64);
                let immediate = self.ins().icmp_imm_u(IntCC::NotEqual, tag, crate::value::ValueLayout::HEAP_TAG as i64);
                self.ins().band(ok, immediate)
            };
            self.ins().brif(ok, direct, &[], general, &[]);
            self.builder.switch_to_block(direct);
            let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
            let word = self.ins().load(types::I64, MemFlagsData::trusted(), object, layout.payload_offset as i32);
            let extras = self.ins().band_imm_u(word, layout.extras_tag_mask as i64);
            let plain_storage = self.ins().icmp_imm_u(IntCC::NotEqual, extras, layout.extras_tag as i64);
            emit_js_guard(&mut self.builder, plain_storage, general);
            let address = emit_arena_slot_address(&mut self.builder, word, properties, properties_len, field.slot, general);
            self.ins().store(MemFlagsData::trusted(), value, address, 0);
            self.ins().jump(done, &[]);
            self.builder.switch_to_block(general);
            self.store_helper(this, atom, instruction.cache_site_index(), value, exit);
            self.ins().jump(done, &[]);
            self.builder.switch_to_block(done);
            return Some(true);
        }
        let this = self.builder.use_var(entry.this);
        self.store_helper(this, atom, instruction.cache_site_index(), value, exit);
        Some(true)
    }

    /// The lane's logged own-field store hit (`Vm::primary_own_field_store_hit`), barrier
    /// included; anything else leaves through `exit`.
    fn store_helper(&mut self, object: ir::Value, atom: Atom, site: u16, value: ir::Value, exit: ir::Block) {
        let atom = self.ins().iconst(types::I64, i64::from(atom));
        let site = self.ins().iconst(types::I64, i64::from(site));
        let stored = emit_lane_helper_call(&mut self.builder, self.payload, self.helpers.field_set, &[object, atom, site, value]);
        let ok = self.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
        emit_js_guard(&mut self.builder, ok, exit);
    }

    fn binary(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, instructions: &[WideInstruction]) -> Option<bool> {
        let returns = instruction.returns_from_frame();
        if instruction.writes_current_this() || (instruction.result_flags() != 0 && !returns) {
            return None;
        }
        // A returning result completes at the terminal `Return`, whose register is free.
        let (destination, after) = if returns {
            let terminal = instructions.len().checked_sub(1)?;
            let last = instructions[terminal];
            (last.op() == Op::Return && last.result_flags() == 0).then_some((last.register_a(), Some(terminal)))?
        } else {
            (instruction.result_register(), None)
        };
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        if !self.operand_supported(left) || !self.operand_supported(right) {
            return None;
        }
        let operator = instruction.binary_operator();
        let arithmetic = super::operations::NumberArithmetic::from_js_instruction(instruction.op(), operator);
        let bitwise = super::operations::NumberBitwise::from_js_instruction(instruction.op(), operator);
        let exit = self.guard_exit(state, pc as u32);
        match (arithmetic, bitwise) {
            (Some(arithmetic), _) if arithmetic != super::operations::NumberArithmetic::Divide => {
                let float = self.builder.create_block();
                self.builder.set_cold_block(float);
                let mut float_state = state.clone();
                let left_int = self.operand_int(state, left, float).expect("supported operand");
                let right_int = self.operand_int(state, right, float).expect("supported operand");
                let (result, overflow) = arithmetic.apply(&mut EmitArithmetic::<true> { builder: &mut self.builder }, left_int, right_int);
                let int_done = self.builder.create_block();
                self.ins().brif(overflow, float, &[], int_done, &[]);
                self.builder.switch_to_block(int_done);
                // A zero product of a negative factor is -0, which int32 cannot hold.
                if arithmetic == super::operations::NumberArithmetic::Multiply {
                    let zero = self.ins().icmp_imm_u(IntCC::Equal, result, 0);
                    let signs = self.ins().bor(left_int, right_int);
                    let negative = self.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
                    let negative_zero = self.ins().band(zero, negative);
                    emit_js_guard_not(&mut self.builder, negative_zero, float);
                }
                let join = self.builder.create_block();
                let bits_param = self.builder.append_block_param(join, types::I64);
                let tagged = int_tagged(&mut self.builder, result);
                self.ins().jump(join, &[ir::BlockArg::Value(tagged)]);
                // The float path: both operands as Numbers, the result boxed.
                self.builder.switch_to_block(float);
                let left_bits = self.operand_tagged(&mut float_state, left).expect("supported operand");
                let right_bits = self.operand_tagged(&mut float_state, right).expect("supported operand");
                let left_number = emit_js_tagged_number(&mut self.builder, left_bits, exit);
                let right_number = emit_js_tagged_number(&mut self.builder, right_bits, exit);
                let number = emit_number_arithmetic_operation(&mut self.builder, arithmetic, left_number, right_number);
                let boxed = emit_js_box_number(&mut self.builder, number);
                self.ins().jump(join, &[ir::BlockArg::Value(boxed)]);
                self.builder.switch_to_block(join);
                // The float path may have boxed operands; keep only what both paths share.
                for (index, known) in float_state.known.iter().enumerate() {
                    if *known != state.known[index] {
                        state.known[index] = Known::Tagged;
                    }
                }
                self.set_tagged(state, destination, bits_param);
            }
            (_, Some(bitwise)) => {
                let left_int = self.operand_int(state, left, exit).expect("supported operand");
                let right_int = self.operand_int(state, right, exit).expect("supported operand");
                if bitwise == super::operations::NumberBitwise::ShiftRightUnsigned {
                    let number = emit_number_bitwise_operation(&mut self.builder, bitwise, left_int, right_int);
                    let boxed = emit_js_box_number(&mut self.builder, number);
                    self.set_tagged(state, destination, boxed);
                } else {
                    let result = emit_number_bitwise_bits(&mut self.builder, bitwise, left_int, right_int);
                    self.set_int(state, destination, result);
                }
            }
            _ => return None,
        }
        if let Some(terminal) = after {
            self.native += 1;
            self.continue_at(state, terminal);
            return Some(false);
        }
        Some(true)
    }
}

/// Continue only when `condition` is false; `fallback` otherwise.
fn emit_js_guard_not(builder: &mut FunctionBuilder<'_>, condition: ir::Value, fallback: ir::Block) {
    let passed = builder.create_block();
    builder.ins().brif(condition, fallback, &[], passed, &[]);
    builder.switch_to_block(passed);
}
