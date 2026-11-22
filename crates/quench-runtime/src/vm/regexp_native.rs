//! Native code for a hot regexp program over an ASCII subject: `quench_regexp`'s plan, one
//! block per instruction, backtracking as a pop of the native stack and a jump table to the
//! frame's resume block. It runs the steps of `Program::run` exactly; the interpreter stays the
//! one source of meaning, and a program or search the code cannot finish keeps it.
//!
//! The emitter is written over the shared machine traits (`staged::machine`) and instantiated on
//! the bare function builder; `emit` is the only place that names the builder.
use super::compiled::CodeMemory;
use super::jit_backend::{compile_function, run_in_background, target_isa, NativeCompileRejection};
use std::task::Poll;
use crate::staged::machine::{Binary, Lane, MachineAccess, MachineFlow, MachineVariables, Relation, Unary};
use cranelift_codegen::ir::{self, types, MemFlagsData};
use cranelift_codegen::isa::TargetIsa;
use cranelift_frontend::FunctionBuilder;
use quench_regexp::backtrack::matcher::{admits, ascii_units_are_words, assertion_holds, takes_unit, word_assertion_given, Admission};
use quench_regexp::backtrack::native::{NativeBuild, NativeCode, NativeCompiler, NativeFn, NativeInst, NativePlan, NativeRestarts, NativeTest, NATIVE_FAIL, NATIVE_FRAME_WORDS, NATIVE_OVERFLOW};

mod look;
mod machine;
mod repeats;
mod stage;

use crate::vm::tier_policy::REGEXP_NATIVE_MAX_CODE_BYTES;
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
    compile_function(function, &*isa, Some(REGEXP_NATIVE_MAX_CODE_BYTES), |compiled, _| {
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
    /// The mark of the lookahead at this pc, its position in the frame's first word: the
    /// backtrack reached it, so the body found no match.
    LookFail(u32),
}

/// What the emitter is written over: a machine with memory, blocks and mutable locals. Every
/// machine that has them is one (`emit_machine` gives the function builder all three).
trait RegexpMachine: MachineAccess + MachineFlow + MachineVariables {}

impl<M: MachineAccess + MachineFlow + MachineVariables> RegexpMachine for M {}

struct Emit<'a, M: RegexpMachine> {
    b: M,
    /// The flags every access carries: the function's trusted memory.
    flags: M::Flags,
    plan: &'a NativePlan,
    bytes: M::Word,
    length: M::Word,
    registers: M::Word,
    stack_end: M::Word,
    /// The last start a search that moves on by itself tries, and where it writes the start
    /// of its match (`NativeFn`).
    last: M::Word,
    found: M::Word,
    position: M::Variable,
    top: M::Variable,
    /// The start of the run, for a search that moves on by itself.
    start: M::Variable,
    /// Each lookahead's mark, by the lookahead's pc: the stack address its body runs above.
    marks: Vec<(u32, M::Variable)>,
    instructions: Vec<M::Block>,
    backtrack: M::Block,
    overflow: M::Block,
    resumes: Vec<(Resume, M::Block)>,
}

fn signature(isa: &dyn TargetIsa) -> ir::Signature {
    crate::vm::compiled::call_signature::word_signature(isa.default_call_conv(), PARAMETERS, 1, types::I64)
}

fn emit(plan: &NativePlan, isa: &dyn TargetIsa) -> ir::Function {
    let mut function = ir::Function::with_name_signature(ir::UserFuncName::default(), signature(isa));
    let mut context = crate::vm::jit_backend::builder::take_context();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    // The entry block takes the function's parameters, which only the builder can append.
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    let parameters: [ir::Value; PARAMETERS] = builder.block_params(entry).try_into().unwrap_or_else(|_| unreachable!("`PARAMETERS` parameters"));
    let mut emit = Emit::<FunctionBuilder<'_>>::start(plan, builder, MemFlagsData::trusted(), entry, parameters);
    for (pc, inst) in plan.insts.iter().enumerate() {
        emit.b.enter_open(emit.instructions[pc]);
        emit.instruction(pc as u32, inst);
    }
    emit.finish();
    let mut builder = emit.into_machine();
    builder.seal_all();
    builder.finalize(isa.frontend_config());
    crate::vm::jit_backend::builder::keep_context(context);
    function
}

impl<'a, M: RegexpMachine> Emit<'a, M> {
    fn start(plan: &'a NativePlan, mut b: M, flags: M::Flags, entry: M::Block, parameters: [M::Word; PARAMETERS]) -> Self {
        b.enter_open(entry);
        let [bytes, length, position_in, registers, stack, stack_end, last, found] = parameters;
        let (position, top, start) = (b.declare_variable(Lane::I64), b.declare_variable(Lane::I64), b.declare_variable(Lane::I64));
        let instructions: Vec<_> = plan.insts.iter().map(|_| b.new_block()).collect();
        let (backtrack, overflow) = (b.new_block(), b.new_block());
        b.define_variable(position, position_in);
        b.define_variable(top, stack);
        // A search that moves on by itself has no entry offset (`Program::native_restarts`).
        b.define_variable(start, position_in);
        let mut emit = Self { b, flags, plan, bytes, length, registers, stack_end, last, found, position, top, start, marks: Vec::new(), instructions, backtrack, overflow, resumes: Vec::new() };
        let barrier = emit.constant(BARRIER);
        let zero = emit.constant(0);
        emit.push_frame(barrier, zero, zero);
        emit.b.goto(emit.instructions[plan.entry as usize], &[]);
        emit
    }

    /// The machine the code was built into, once every instruction is emitted.
    fn into_machine(self) -> M {
        self.b
    }

    fn constant(&mut self, value: i64) -> M::Word {
        self.b.raw_const(Lane::I64, value)
    }

    fn position(&mut self) -> M::Word {
        self.b.read_variable(self.position)
    }

    fn set_position(&mut self, value: M::Word) {
        self.b.define_variable(self.position, value);
    }

    fn top(&mut self) -> M::Word {
        self.b.read_variable(self.top)
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
    fn branch(&mut self, condition: M::Word, then: M::Block, otherwise: M::Block) {
        self.b.branch_if(condition, (then, &[]), (otherwise, &[]));
    }

    fn compare(&mut self, relation: Relation, left: M::Word, right: M::Word) -> M::Word {
        self.b.raw_compare(relation, left, right)
    }

    fn compare_constant(&mut self, relation: Relation, value: M::Word, constant: i64) -> M::Word {
        self.b.raw_compare_imm(relation, value, constant)
    }

    fn register(&mut self, index: u32) -> M::Word {
        self.b.raw_load(Lane::I64, self.flags, self.registers, index as i32 * WORD)
    }

    fn store_register(&mut self, index: u32, value: M::Word) {
        self.b.raw_store(Lane::I64, self.flags, (value, false), self.registers, index as i32 * WORD);
    }

    /// Pushes a frame `(id, first, second)`, returning from the search at once where the
    /// stack is full.
    fn push_frame(&mut self, id: M::Word, first: M::Word, second: M::Word) {
        let top = self.top();
        let after = self.b.raw_binary_imm(Binary::Add, top, FRAME_BYTES);
        let fits = self.compare(Relation::UnsignedLessEqual, after, self.stack_end);
        let stored = self.b.new_block();
        self.branch(fits, stored, self.overflow);
        self.b.enter_open(stored);
        let flags = self.flags;
        self.b.raw_store(Lane::I64, flags, (id, false), top, 0);
        self.b.raw_store(Lane::I64, flags, (first, false), top, FIRST_OPERAND);
        self.b.raw_store(Lane::I64, flags, (second, false), top, SECOND_OPERAND);
        self.b.define_variable(self.top, after);
    }

    fn push_resume(&mut self, resume: Resume, first: M::Word, second: M::Word) {
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
        self.b.define_variable(self.top, top);
        let id = self.b.raw_load(Lane::I64, self.flags, top, 0);
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
        let start = self.b.read_variable(self.start);
        let start = self.b.raw_binary_imm(Binary::Add, start, 1);
        self.b.define_variable(self.start, start);
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
        self.run_from_start(admission, next);
    }

    /// Runs from the restart's start, `admission` having admitted it: past its leading
    /// assertions, which fail on to `next` with no frame of their own to undo, and above a
    /// new barrier.
    fn run_from_start(&mut self, admission: Option<Admission<NativeTest>>, next: M::Block) {
        let start = self.b.read_variable(self.start);
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
        self.b.raw_store(Lane::I64, self.flags, (start, false), self.found, 0);
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
        let at = self.b.read_variable(self.start);
        let takes = takes_unit(self, test, at);
        self.branch(takes, step, done);
        self.b.enter_open(step);
        let after = self.b.raw_binary_imm(Binary::Add, at, 1);
        self.b.define_variable(self.start, after);
        self.b.goto(scan, &[]);
        self.b.enter_open(done);
    }

    /// Jumps by `id` to the restore block, the barrier block or a resume block.
    fn dispatch(&mut self, id: M::Word, restore: M::Block, barrier: M::Block) {
        let mut targets = vec![restore, barrier];
        targets.extend(self.resumes.iter().map(|(_, block)| *block));
        // No frame has another id; the search is given back to the interpreter if one did.
        let table = self.b.jump_table(&targets, (self.overflow, &[]));
        let selector = self.b.raw_unary(Unary::Narrow(Lane::I32), id);
        self.b.dispatch(selector, table);
    }

    /// A restore frame: its register gets its old value back, and the backtrack goes on.
    fn restore_frame(&mut self, frame: M::Word) {
        self.write_back(frame);
        self.fail();
    }

    /// The register a restore entry at `frame` names gets its old value back.
    fn write_back(&mut self, frame: M::Word) {
        let flags = self.flags;
        let register = self.b.raw_load(Lane::I64, flags, frame, FIRST_OPERAND);
        let value = self.b.raw_load(Lane::I64, flags, frame, SECOND_OPERAND);
        let offset = self.b.raw_binary_imm(Binary::ShiftLeft, register, REGISTER_SHIFT);
        let address = self.b.raw_binary(Binary::Add, self.registers, offset);
        self.b.raw_store(Lane::I64, flags, (value, false), address, 0);
    }
}

#[cfg(test)]
mod tests;
