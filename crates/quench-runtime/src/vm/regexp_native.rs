//! Native code for a hot regexp program over an ASCII subject: `quench_regexp`'s plan, one
//! Cranelift block per instruction, backtracking as a pop of the native stack and a jump table
//! to the frame's resume block. It runs the steps of `Program::run` exactly; the interpreter
//! stays the one source of meaning, and a program or search the code cannot finish keeps it.
use super::jit_backend::{compile_function, run_in_background, target_isa, NativeCompileRejection};
use std::task::Poll;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlagsData};
use cranelift_codegen::isa::TargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use quench_regexp::backtrack::native::{NativeBuild, NativeCode, NativeCompiler, NativeFn, NativeGuard, NativeInst, NativePlan, NativeTest, NATIVE_FAIL, NATIVE_FRAME_WORDS, NATIVE_OVERFLOW};

mod assertions;
mod repeats;

/// The largest native search, in bytes of machine code.
const MAX_CODE_BYTES: usize = 256 * 1024;
const WORD: i32 = std::mem::size_of::<usize>() as i32;
const FRAME_BYTES: i64 = (NATIVE_FRAME_WORDS * std::mem::size_of::<usize>()) as i64;
/// Frame ids below `FIRST_RESUME` are the frames the backtrack block handles itself.
const RESTORE: i64 = 0;
const BARRIER: i64 = 1;
const FIRST_RESUME: i64 = 2;
/// The words of a frame after its id: two operands.
const FIRST_OPERAND: i32 = WORD;
const SECOND_OPERAND: i32 = 2 * WORD;
/// The parameters of a native search (`NativeFn`).
const PARAMETERS: usize = 6;
/// An address of register `n` is `n << REGISTER_SHIFT` bytes on.
const REGISTER_SHIFT: i64 = 3;
/// A capture's two registers.
const CAPTURE_REGISTERS: u32 = 2;
/// The bits of a mask word, and the mask of a shift within one.
const WORD_BITS: i64 = 64;
const WORD_SHIFT_MASK: i64 = WORD_BITS - 1;

/// Whether this thread's hot regexp programs run natively: only under a compiling mode.
pub(super) fn enable(enabled: bool) {
    quench_regexp::backtrack::native::set_native_compiler(compiler());
    quench_regexp::backtrack::native::enable_native_search(enabled);
}

/// The compiler `quench_regexp` asks for code.
pub(super) fn compiler() -> NativeCompiler {
    NativeCompiler { runs: super::compiled::tier_policy::REGEXP_NATIVE_RUNS, start }
}

/// Compiles `plan` on the code generator's thread; the program polls for the code and keeps
/// the interpreter meanwhile.
fn start(plan: NativePlan) -> NativeBuild {
    let pending = run_in_background(move || compile(&plan));
    Box::new(move || match pending.poll() {
        Poll::Pending => Poll::Pending,
        Poll::Ready(bytes) => Poll::Ready(bytes.flatten().and_then(|bytes| publish(&bytes))),
    })
}

/// The machine code of `plan`.
fn compile(plan: &NativePlan) -> Option<Vec<u8>> {
    let isa = target_isa()?;
    if isa.pointer_type() != types::I64 {
        return None;
    }
    let function = emit(plan, &*isa);
    compile_function(function, &*isa, Some(MAX_CODE_BYTES), |compiled, _| {
        if !compiled.buffer.relocs().is_empty() {
            return Err(NativeCompileRejection::Relocations);
        }
        Ok(compiled.code_buffer().to_vec())
    })
    .ok()
}

/// `code` in executable memory, as a native search (`publish_entry`).
fn publish(code: &[u8]) -> Option<NativeCode> {
    let (memory, entry) = super::compiled::publish_entry::<NativeFn>(code)?;
    Some(NativeCode { entry, keep: Box::new(memory) })
}

/// How a resume frame continues: the frame's id names one of these.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Resume {
    /// A choice point at an instruction, with the position in the frame's last word.
    Choice(u32),
    /// A greedy repeat that gives back a unit, floor and position in the frame.
    Greedy(u32),
    /// A lazy repeat that takes a unit, ceiling and position in the frame.
    Lazy(u32),
}

struct Emit<'a, 'f> {
    b: FunctionBuilder<'f>,
    plan: &'a NativePlan,
    bytes: ir::Value,
    length: ir::Value,
    registers: ir::Value,
    stack_end: ir::Value,
    position: Variable,
    top: Variable,
    instructions: Vec<ir::Block>,
    backtrack: ir::Block,
    overflow: ir::Block,
    resumes: Vec<(Resume, ir::Block)>,
}

fn signature(isa: &dyn TargetIsa) -> ir::Signature {
    let mut signature = ir::Signature::new(isa.default_call_conv());
    for _ in 0..PARAMETERS {
        signature.params.push(ir::AbiParam::new(types::I64));
    }
    signature.returns.push(ir::AbiParam::new(types::I64));
    signature
}

fn emit(plan: &NativePlan, isa: &dyn TargetIsa) -> ir::Function {
    let mut function = ir::Function::with_name_signature(ir::UserFuncName::default(), signature(isa));
    let mut context = FunctionBuilderContext::new();
    let mut emit = Emit::start(plan, FunctionBuilder::new(&mut function, &mut context));
    for (pc, inst) in plan.insts.iter().enumerate() {
        emit.b.switch_to_block(emit.instructions[pc]);
        emit.instruction(pc as u32, inst);
    }
    emit.finish();
    emit.b.seal_all_blocks();
    emit.b.finalize(isa.frontend_config());
    function
}

impl<'a, 'f> Emit<'a, 'f> {
    fn start(plan: &'a NativePlan, mut b: FunctionBuilder<'f>) -> Self {
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let parameters = b.block_params(entry).to_vec();
        let [bytes, length, position_in, registers, stack, stack_end] = parameters[..] else { unreachable!("`PARAMETERS` parameters") };
        let (position, top) = (b.declare_var(types::I64), b.declare_var(types::I64));
        let instructions: Vec<_> = plan.insts.iter().map(|_| b.create_block()).collect();
        let (backtrack, overflow) = (b.create_block(), b.create_block());
        b.def_var(position, position_in);
        b.def_var(top, stack);
        let mut emit = Self { b, plan, bytes, length, registers, stack_end, position, top, instructions, backtrack, overflow, resumes: Vec::new() };
        let barrier = emit.constant(BARRIER);
        let zero = emit.constant(0);
        emit.push_frame(barrier, zero, zero);
        emit.b.ins().jump(emit.instructions[plan.entry as usize], &[]);
        emit
    }

    fn constant(&mut self, value: i64) -> ir::Value {
        self.b.ins().iconst(types::I64, value)
    }

    fn position(&mut self) -> ir::Value {
        self.b.use_var(self.position)
    }

    fn set_position(&mut self, value: ir::Value) {
        self.b.def_var(self.position, value);
    }

    fn top(&mut self) -> ir::Value {
        self.b.use_var(self.top)
    }

    fn next(&mut self, pc: u32) {
        self.b.ins().jump(self.instructions[pc as usize + 1], &[]);
    }

    fn goto(&mut self, pc: u32) {
        self.b.ins().jump(self.instructions[pc as usize], &[]);
    }

    fn fail(&mut self) {
        self.b.ins().jump(self.backtrack, &[]);
    }

    /// Branches to `then` when `condition` holds, else `otherwise`.
    fn branch(&mut self, condition: ir::Value, then: ir::Block, otherwise: ir::Block) {
        self.b.ins().brif(condition, then, &[], otherwise, &[]);
    }

    /// Continues in a new block where `condition` holds, failing where it does not.
    fn require(&mut self, condition: ir::Value) {
        let held = self.b.create_block();
        self.branch(condition, held, self.backtrack);
        self.b.switch_to_block(held);
    }

    fn compare(&mut self, condition: IntCC, left: ir::Value, right: ir::Value) -> ir::Value {
        self.b.ins().icmp(condition, left, right)
    }

    fn compare_constant(&mut self, condition: IntCC, value: ir::Value, constant: i64) -> ir::Value {
        self.b.ins().icmp_imm_u(condition, value, constant)
    }

    fn byte_at(&mut self, index: ir::Value) -> ir::Value {
        let address = self.b.ins().iadd(self.bytes, index);
        self.b.ins().uload8(types::I64, MemFlagsData::trusted(), address, 0)
    }

    fn register(&mut self, index: u32) -> ir::Value {
        self.b.ins().load(types::I64, MemFlagsData::trusted(), self.registers, index as i32 * WORD)
    }

    fn store_register(&mut self, index: u32, value: ir::Value) {
        self.b.ins().store(MemFlagsData::trusted(), value, self.registers, index as i32 * WORD);
    }

    /// Whether the unit `byte` passes `test`.
    fn passes(&mut self, test: NativeTest, byte: ir::Value) -> ir::Value {
        match test {
            NativeTest::Never => self.b.ins().iconst(types::I8, 0),
            NativeTest::Exact(unit) => self.compare_constant(IntCC::Equal, byte, i64::from(unit)),
            NativeTest::Mask(mask) => self.in_mask(mask, byte),
        }
    }

    /// Bit `byte` of the 128-bit `mask`.
    fn in_mask(&mut self, mask: [u64; 2], byte: ir::Value) -> ir::Value {
        let low = self.b.ins().iconst(types::I64, mask[0] as i64);
        let high = self.b.ins().iconst(types::I64, mask[1] as i64);
        let below = self.compare_constant(IntCC::UnsignedLessThan, byte, WORD_BITS);
        let word = self.b.ins().select(below, low, high);
        let shift = self.b.ins().band_imm_u(byte, WORD_SHIFT_MASK);
        let bit = self.b.ins().ushr(word, shift);
        let bit = self.b.ins().band_imm_u(bit, 1);
        self.compare_constant(IntCC::NotEqual, bit, 0)
    }

    /// Pushes a frame `(id, first, second)`, returning from the search at once where the
    /// stack is full.
    fn push_frame(&mut self, id: ir::Value, first: ir::Value, second: ir::Value) {
        let top = self.top();
        let after = self.b.ins().iadd_imm_s(top, FRAME_BYTES);
        let fits = self.compare(IntCC::UnsignedLessThanOrEqual, after, self.stack_end);
        let stored = self.b.create_block();
        self.branch(fits, stored, self.overflow);
        self.b.switch_to_block(stored);
        let flags = MemFlagsData::trusted();
        self.b.ins().store(flags, id, top, 0);
        self.b.ins().store(flags, first, top, FIRST_OPERAND);
        self.b.ins().store(flags, second, top, SECOND_OPERAND);
        self.b.def_var(self.top, after);
    }

    fn push_resume(&mut self, resume: Resume, first: ir::Value, second: ir::Value) {
        let id = self.resume_id(resume);
        let id = self.constant(id);
        self.push_frame(id, first, second);
    }

    /// The id of a resume, with its block, made once.
    fn resume_id(&mut self, resume: Resume) -> i64 {
        let index = self.resumes.iter().position(|(known, _)| *known == resume).unwrap_or_else(|| {
            let block = self.b.create_block();
            self.resumes.push((resume, block));
            self.resumes.len() - 1
        });
        FIRST_RESUME + index as i64
    }

    /// Records the register's old value so backtracking puts it back, then sets it.
    fn set_register(&mut self, index: u32, value: ir::Value) {
        let old = self.register(index);
        let id = self.constant(RESTORE);
        let register = self.constant(i64::from(index));
        self.push_frame(id, register, old);
        self.store_register(index, value);
    }

    /// `set_register` of both bounds of a capture.
    fn set_capture(&mut self, capture: u32, start: ir::Value, end: ir::Value) {
        self.set_register(CAPTURE_REGISTERS * capture, start);
        self.set_register(CAPTURE_REGISTERS * capture + 1, end);
    }

    fn instruction(&mut self, pc: u32, inst: &NativeInst) {
        match inst {
            NativeInst::Unit(test) => self.unit(pc, *test),
            NativeInst::UnitRepeat { test, min, max, greedy } => self.unit_repeat(pc, *test, (*min, *max), *greedy),
            NativeInst::Assert(assertion) => self.assertion(pc, *assertion),
            NativeInst::Split { alternative } => {
                let position = self.position();
                self.push_resume(Resume::Choice(*alternative), position, position);
                self.next(pc);
            }
            NativeInst::GuardedSplit { alternative, guard } => self.guarded_split(pc, *alternative, *guard),
            NativeInst::Jump(target) => self.goto(*target),
            NativeInst::Match => {
                let position = self.position();
                self.b.ins().return_(&[position]);
            }
            other => self.registers_instruction(pc, other),
        }
    }

    fn unit(&mut self, pc: u32, test: NativeTest) {
        let position = self.position();
        let inside = self.compare(IntCC::UnsignedLessThan, position, self.length);
        self.require(inside);
        let byte = self.byte_at(position);
        let passes = self.passes(test, byte);
        self.require(passes);
        let after = self.b.ins().iadd_imm_s(position, 1);
        self.set_position(after);
        self.next(pc);
    }

    fn guarded_split(&mut self, pc: u32, alternative: u32, guard: NativeGuard) {
        let (admit, refuse) = (self.b.create_block(), self.b.create_block());
        let position = self.position();
        if guard.at_start {
            let probe = self.b.create_block();
            let at_start = self.compare_constant(IntCC::Equal, position, 0);
            self.branch(at_start, admit, probe);
            self.b.switch_to_block(probe);
        }
        let (load, edge) = (self.b.create_block(), self.b.create_block());
        let inside = self.compare(IntCC::UnsignedLessThan, position, self.length);
        self.branch(inside, load, edge);
        self.b.switch_to_block(load);
        let byte = self.byte_at(position);
        let starts = self.in_mask(guard.mask, byte);
        self.branch(starts, admit, refuse);
        self.b.switch_to_block(edge);
        self.b.ins().jump(if guard.at_end { admit } else { refuse }, &[]);
        self.b.switch_to_block(admit);
        self.push_resume(Resume::Choice(alternative), position, position);
        self.next(pc);
        self.b.switch_to_block(refuse);
        self.goto(alternative);
    }

    /// The instructions that read and write registers, the counters and captures of the loops.
    fn registers_instruction(&mut self, pc: u32, inst: &NativeInst) {
        match inst {
            NativeInst::CaptureOpen { temporary, restores } => {
                let position = self.position();
                if *restores {
                    self.set_register(*temporary, position);
                } else {
                    self.store_register(*temporary, position);
                }
                self.next(pc);
            }
            NativeInst::CaptureClose { capture, temporary } => {
                let start = self.register(*temporary);
                let end = self.position();
                self.set_capture(*capture, start, end);
                self.next(pc);
            }
            NativeInst::ResetCaptures { captures } => self.reset_captures(pc, captures),
            other => self.loop_instruction(pc, other),
        }
    }

    fn reset_captures(&mut self, pc: u32, captures: &[u32]) {
        let unset = self.constant(self.plan.unset as i64);
        for capture in captures {
            let current = self.register(CAPTURE_REGISTERS * capture);
            let is_set = self.compare(IntCC::NotEqual, current, unset);
            let (reset, skip) = (self.b.create_block(), self.b.create_block());
            self.branch(is_set, reset, skip);
            self.b.switch_to_block(reset);
            self.set_capture(*capture, unset, unset);
            self.b.ins().jump(skip, &[]);
            self.b.switch_to_block(skip);
        }
        self.next(pc);
    }

    /// The backtrack block: pops a frame and goes where its id says; then each resume block.
    fn finish(&mut self) {
        let overflow = self.overflow;
        self.b.switch_to_block(overflow);
        let overflowed = self.constant(NATIVE_OVERFLOW as i64);
        self.b.ins().return_(&[overflowed]);
        let (restore, barrier) = (self.b.create_block(), self.b.create_block());
        self.b.switch_to_block(self.backtrack);
        let top = self.top();
        let top = self.b.ins().iadd_imm_s(top, -FRAME_BYTES);
        self.b.def_var(self.top, top);
        let id = self.b.ins().load(types::I64, MemFlagsData::trusted(), top, 0);
        self.dispatch(id, restore, barrier);
        self.b.switch_to_block(restore);
        self.restore_frame(top);
        self.b.switch_to_block(barrier);
        let failed = self.constant(NATIVE_FAIL as i64);
        self.b.ins().return_(&[failed]);
        // The resume blocks stay listed: a resume pushes frames of its own kind, which name
        // their ids from the list (`resume_id`), so it never grows here.
        let listed = self.resumes.len();
        for index in 0..listed {
            let (resume, block) = self.resumes[index];
            self.b.switch_to_block(block);
            self.resume(resume);
        }
        debug_assert_eq!(self.resumes.len(), listed, "no resume made another");
    }

    /// Jumps by `id` to the restore block, the barrier block or a resume block.
    fn dispatch(&mut self, id: ir::Value, restore: ir::Block, barrier: ir::Block) {
        let mut targets = vec![restore, barrier];
        targets.extend(self.resumes.iter().map(|(_, block)| *block));
        let cases: Vec<_> = targets.iter().map(|target| self.b.func.dfg.block_call(*target, &[])).collect();
        // No frame has another id; the search is given back to the interpreter if one did.
        let default = self.b.func.dfg.block_call(self.overflow, &[]);
        let table = self.b.create_jump_table(ir::JumpTableData::new(default, &cases));
        let selector = self.b.ins().ireduce(types::I32, id);
        self.b.ins().br_table(selector, table);
    }

    /// A restore frame: its register gets its old value back, and the backtrack goes on.
    fn restore_frame(&mut self, frame: ir::Value) {
        let flags = MemFlagsData::trusted();
        let register = self.b.ins().load(types::I64, flags, frame, FIRST_OPERAND);
        let value = self.b.ins().load(types::I64, flags, frame, SECOND_OPERAND);
        let offset = self.b.ins().ishl_imm_u(register, REGISTER_SHIFT);
        let address = self.b.ins().iadd(self.registers, offset);
        self.b.ins().store(flags, value, address, 0);
        self.fail();
    }
}

#[cfg(test)]
mod tests;
