//! Native code for a hot regexp program over an ASCII subject: `quench_regexp`'s plan, one
//! Cranelift block per instruction, backtracking as a pop of the native stack and a jump table
//! to the frame's resume block. It runs the steps of `Program::run` exactly; the interpreter
//! stays the one source of meaning, and a program or search the code cannot finish keeps it.
use super::compiled::CodeMemory;
use super::jit_backend::{compile_function, run_in_background, target_isa, NativeCompileRejection};
use std::task::Poll;
use crate::staged::machine::{Binary, Lane, MachineAccess, MachineFlow, MachineScalar, Relation, Unary};
use cranelift_codegen::ir::{self, types, MemFlagsData};
use cranelift_codegen::isa::TargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use quench_regexp::backtrack::CAPTURE_REGISTERS;
use quench_regexp::backtrack::matcher::{admits, ascii_units_are_words, assertion_holds, takes_unit, word_assertion_given, Admission, AssertionTest};
use quench_regexp::backtrack::native::{NativeBuild, NativeCode, NativeCompiler, NativeFn, NativeInst, NativePlan, NativeRestarts, NativeTest, NATIVE_FAIL, NATIVE_FRAME_WORDS, NATIVE_OVERFLOW};

mod repeats;
mod stage;

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
const PARAMETERS: usize = 8;
/// An address of register `n` is `n << REGISTER_SHIFT` bytes on.
const REGISTER_SHIFT: i64 = 3;

/// Whether this thread's hot regexp programs run natively: only under a compiling mode.
pub(super) fn enable(enabled: bool, runs: u32) {
    quench_regexp::backtrack::native::set_native_compiler(compiler(runs));
    quench_regexp::backtrack::native::enable_native_search(enabled);
}

/// The compiler `quench_regexp` asks for code.
fn compiler(runs: u32) -> NativeCompiler {
    NativeCompiler { runs, start }
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

/// `code` in executable memory, as a native search.
fn publish(code: &[u8]) -> Option<NativeCode> {
    let memory = CodeMemory::new(code)?;
    // SAFETY: the code is a function of `NativeFn`'s signature, which `emit` builds.
    let entry = unsafe { std::mem::transmute::<*mut u8, NativeFn>(memory.pointer()) };
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
    /// The last start a search that moves on by itself tries, and where it writes the start
    /// of its match (`NativeFn`).
    last: ir::Value,
    found: ir::Value,
    position: Variable,
    top: Variable,
    /// The start of the run, for a search that moves on by itself.
    start: Variable,
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
        emit.b.enter_open(emit.instructions[pc]);
        emit.instruction(pc as u32, inst);
    }
    emit.finish();
    emit.b.seal_all();
    emit.b.finalize(isa.frontend_config());
    function
}

impl<'a, 'f> Emit<'a, 'f> {
    fn start(plan: &'a NativePlan, mut b: FunctionBuilder<'f>) -> Self {
        let entry = b.new_block();
        b.append_block_params_for_function_params(entry);
        b.enter_open(entry);
        let parameters = b.block_params(entry).to_vec();
        let [bytes, length, position_in, registers, stack, stack_end, last, found] = parameters[..] else { unreachable!("`PARAMETERS` parameters") };
        let (position, top, start) = (b.declare_var(types::I64), b.declare_var(types::I64), b.declare_var(types::I64));
        let instructions: Vec<_> = plan.insts.iter().map(|_| b.new_block()).collect();
        let (backtrack, overflow) = (b.new_block(), b.new_block());
        b.def_var(position, position_in);
        b.def_var(top, stack);
        // A search that moves on by itself has no entry offset (`Program::native_restarts`).
        b.def_var(start, position_in);
        let mut emit = Self { b, plan, bytes, length, registers, stack_end, last, found, position, top, start, instructions, backtrack, overflow, resumes: Vec::new() };
        let barrier = emit.constant(BARRIER);
        let zero = emit.constant(0);
        emit.push_frame(barrier, zero, zero);
        emit.b.goto(emit.instructions[plan.entry as usize], &[]);
        emit
    }

    fn constant(&mut self, value: i64) -> ir::Value {
        self.b.raw_const(Lane::I64, value)
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
        self.b.goto(self.instructions[pc as usize + 1], &[]);
    }

    fn goto(&mut self, pc: u32) {
        self.b.goto(self.instructions[pc as usize], &[]);
    }

    fn fail(&mut self) {
        self.b.goto(self.backtrack, &[]);
    }

    /// Branches to `then` when `condition` holds, else `otherwise`.
    fn branch(&mut self, condition: ir::Value, then: ir::Block, otherwise: ir::Block) {
        self.b.branch_if(condition, (then, &[]), (otherwise, &[]));
    }

    /// Continues in a new block where `condition` holds, failing where it does not.
    fn require(&mut self, condition: ir::Value) {
        let held = self.b.new_block();
        self.branch(condition, held, self.backtrack);
        self.b.enter_open(held);
    }

    fn compare(&mut self, relation: Relation, left: ir::Value, right: ir::Value) -> ir::Value {
        self.b.raw_compare(relation, left, right)
    }

    fn compare_constant(&mut self, relation: Relation, value: ir::Value, constant: i64) -> ir::Value {
        self.b.raw_compare_imm(relation, value, constant)
    }

    fn register(&mut self, index: u32) -> ir::Value {
        self.b.raw_load(Lane::I64, MemFlagsData::trusted(), self.registers, index as i32 * WORD)
    }

    fn store_register(&mut self, index: u32, value: ir::Value) {
        self.b.raw_store(Lane::I64, MemFlagsData::trusted(), (value, false), self.registers, index as i32 * WORD);
    }

    /// Pushes a frame `(id, first, second)`, returning from the search at once where the
    /// stack is full.
    fn push_frame(&mut self, id: ir::Value, first: ir::Value, second: ir::Value) {
        let top = self.top();
        let after = self.b.raw_binary_imm(Binary::Add, top, FRAME_BYTES);
        let fits = self.compare(Relation::UnsignedLessEqual, after, self.stack_end);
        let stored = self.b.new_block();
        self.branch(fits, stored, self.overflow);
        self.b.enter_open(stored);
        let flags = MemFlagsData::trusted();
        self.b.raw_store(Lane::I64, flags, (id, false), top, 0);
        self.b.raw_store(Lane::I64, flags, (first, false), top, FIRST_OPERAND);
        self.b.raw_store(Lane::I64, flags, (second, false), top, SECOND_OPERAND);
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
            let block = self.b.new_block();
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
            NativeInst::UnitRepeat { test, min, max, greedy, atomic } => self.unit_repeat(pc, *test, (*min, *max), (*greedy, *atomic)),
            NativeInst::Assert(test) => self.assertion(pc, *test),
            NativeInst::Split { alternative } => {
                let position = self.position();
                self.push_resume(Resume::Choice(*alternative), position, position);
                self.next(pc);
            }
            NativeInst::GuardedSplit { alternative, guard } => self.guarded_split(pc, *alternative, *guard),
            NativeInst::Jump(target) => self.goto(*target),
            NativeInst::Match => {
                let position = self.position();
                self.b.leave(&[position]);
            }
            other => self.registers_instruction(pc, other),
        }
    }

    fn unit(&mut self, pc: u32, test: NativeTest) {
        let position = self.position();
        let takes = takes_unit(self, test, position);
        self.require(takes);
        let after = self.b.raw_binary_imm(Binary::Add, position, 1);
        self.set_position(after);
        self.next(pc);
    }

    fn assertion(&mut self, pc: u32, test: AssertionTest) {
        let position = self.position();
        let holds = assertion_holds(self, test, position);
        self.require(holds);
        self.next(pc);
    }

    fn guarded_split(&mut self, pc: u32, alternative: u32, guard: Admission<NativeTest>) {
        let position = self.position();
        let admitted = admits(self, guard, position);
        let (admit, refuse) = (self.b.new_block(), self.b.new_block());
        self.branch(admitted, admit, refuse);
        self.b.enter_open(admit);
        self.push_resume(Resume::Choice(alternative), position, position);
        self.next(pc);
        self.b.enter_open(refuse);
        self.goto(alternative);
    }

    /// The instructions that read and write registers, the counters and captures of the loops.
    fn registers_instruction(&mut self, pc: u32, inst: &NativeInst) {
        match inst {
            NativeInst::CaptureOpen { temporary, count, restores } => {
                let position = self.position();
                for register in *temporary..*temporary + *count {
                    if *restores {
                        self.set_register(register, position);
                    } else {
                        self.store_register(register, position);
                    }
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
            let is_set = self.compare(Relation::NotEqual, current, unset);
            let (reset, skip) = (self.b.new_block(), self.b.new_block());
            self.branch(is_set, reset, skip);
            self.b.enter_open(reset);
            self.set_capture(*capture, unset, unset);
            self.b.goto(skip, &[]);
            self.b.enter_open(skip);
        }
        self.next(pc);
    }

    /// The backtrack block: pops a frame and goes where its id says; then each resume block.
    fn finish(&mut self) {
        let overflow = self.overflow;
        self.b.enter_open(overflow);
        let overflowed = self.constant(NATIVE_OVERFLOW as i64);
        self.b.leave(&[overflowed]);
        let (restore, barrier) = (self.b.new_block(), self.b.new_block());
        self.b.enter_open(self.backtrack);
        let top = self.top();
        let top = self.b.raw_binary_imm(Binary::Add, top, -FRAME_BYTES);
        self.b.def_var(self.top, top);
        let id = self.b.raw_load(Lane::I64, MemFlagsData::trusted(), top, 0);
        self.dispatch(id, restore, barrier);
        self.b.enter_open(restore);
        self.restore_frame(top);
        self.b.enter_open(barrier);
        self.restart();
        // The resume blocks stay listed: a resume pushes frames of its own kind, which name
        // their ids from the list (`resume_id`), so it never grows here.
        let listed = self.resumes.len();
        for index in 0..listed {
            let (resume, block) = self.resumes[index];
            self.b.enter_open(block);
            self.resume(resume);
        }
        debug_assert_eq!(self.resumes.len(), listed, "no resume made another");
    }

    /// The barrier: the run from this start failed. The caller finds the next start, or the
    /// code moves to the next position past it that the start set admits (`Program::next_start`)
    /// and runs from there, failing past `last`.
    fn restart(&mut self) {
        let admission = match self.plan.restarts {
            NativeRestarts::Caller => None,
            NativeRestarts::Every => Some(None),
            NativeRestarts::Admitted(admission) => Some(Some(admission)),
        };
        let Some(admission) = admission else {
            let failed = self.constant(NATIVE_FAIL as i64);
            self.b.leave(&[failed]);
            return;
        };
        let (next, exhausted, run) = (self.b.new_block(), self.b.new_block(), self.b.new_block());
        if let Some(test) = self.plan.restart_skip {
            self.skip_taken_units(test);
        }
        self.b.goto(next, &[]);
        self.b.enter_open(next);
        let start = self.b.use_var(self.start);
        let start = self.b.raw_binary_imm(Binary::Add, start, 1);
        self.b.def_var(self.start, start);
        let past = self.compare(Relation::UnsignedGreater, start, self.last);
        let candidate = self.b.new_block();
        self.branch(past, exhausted, candidate);
        self.b.enter_open(candidate);
        match admission {
            None => self.b.goto(run, &[]),
            Some(admission) => {
                let admitted = admits(self, admission, start);
                self.branch(admitted, run, next);
            }
        }
        self.b.enter_open(exhausted);
        let failed = self.constant(NATIVE_FAIL as i64);
        self.b.leave(&[failed]);
        self.b.enter_open(run);
        let start = self.b.use_var(self.start);
        // A leading assertion that fails fails the run with no frame of its own to undo: it
        // is decided here, before the run's barrier.
        // The start set may say whether the unit at the start is a word unit, which leaves
        // `\b` and `\B` only the unit before it to read.
        let word = match admission {
            Some(Admission { units: NativeTest::Mask(mask), at_end, .. }) => ascii_units_are_words(mask).filter(|word| !(*word && at_end)),
            _ => None,
        };
        let mut entry = self.plan.entry;
        while let Some(NativeInst::Assert(test)) = self.plan.insts.get(entry as usize) {
            let given = word.and_then(|word| word_assertion_given(self, *test, start, word));
            let holds = given.unwrap_or_else(|| assertion_holds(self, *test, start));
            let held = self.b.new_block();
            self.branch(holds, held, next);
            self.b.enter_open(held);
            entry += 1;
        }
        self.b.raw_store(Lane::I64, MemFlagsData::trusted(), (start, false), self.found, 0);
        self.set_position(start);
        let barrier = self.constant(BARRIER);
        let zero = self.constant(0);
        self.push_frame(barrier, zero, zero);
        self.goto(entry);
    }

    /// Moves the failed start to the end of the units its leading atomic repeat took
    /// (`Program::next_candidate`), whose next position the restart tries.
    fn skip_taken_units(&mut self, test: NativeTest) {
        let (scan, step, done) = (self.b.new_block(), self.b.new_block(), self.b.new_block());
        self.b.goto(scan, &[]);
        self.b.enter_open(scan);
        let at = self.b.use_var(self.start);
        let takes = takes_unit(self, test, at);
        self.branch(takes, step, done);
        self.b.enter_open(step);
        let after = self.b.raw_binary_imm(Binary::Add, at, 1);
        self.b.def_var(self.start, after);
        self.b.goto(scan, &[]);
        self.b.enter_open(done);
    }

    /// Jumps by `id` to the restore block, the barrier block or a resume block.
    fn dispatch(&mut self, id: ir::Value, restore: ir::Block, barrier: ir::Block) {
        let mut targets = vec![restore, barrier];
        targets.extend(self.resumes.iter().map(|(_, block)| *block));
        // No frame has another id; the search is given back to the interpreter if one did.
        let table = self.b.jump_table(&targets, (self.overflow, &[]));
        let selector = self.b.raw_unary(Unary::Narrow(Lane::I32), id);
        self.b.dispatch(selector, table);
    }

    /// A restore frame: its register gets its old value back, and the backtrack goes on.
    fn restore_frame(&mut self, frame: ir::Value) {
        let flags = MemFlagsData::trusted();
        let register = self.b.raw_load(Lane::I64, flags, frame, FIRST_OPERAND);
        let value = self.b.raw_load(Lane::I64, flags, frame, SECOND_OPERAND);
        let offset = self.b.raw_binary_imm(Binary::ShiftLeft, register, REGISTER_SHIFT);
        let address = self.b.raw_binary(Binary::Add, self.registers, offset);
        self.b.raw_store(Lane::I64, flags, (value, false), address, 0);
        self.fail();
    }
}

#[cfg(test)]
mod tests;
