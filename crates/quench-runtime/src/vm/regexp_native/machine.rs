//! `Machine` on the emitter: the VM's instructions are `quench_regexp::backtrack::machine`'s
//! functions, the interpreter's own, run here with the program's operands static, so each
//! instruction's code is derived from the semantics rather than written beside it. A flag the
//! machine requires branches to the backtrack block; a choice branches and finishes both paths.
use super::{Emit, Resume, RESTORE};
use crate::staged::machine::{Binary, MachineFlow, MachineScalar, Relation};
use cranelift_codegen::ir;
use quench_regexp::backtrack::machine::{self as m, Cursor, Machine, Step, CAPTURE_REGISTERS, UNBOUNDED};
use quench_regexp::backtrack::native::{NativeInst, NativeTest};

impl Emit<'_, '_> {
    /// The code of the instruction at `pc`.
    pub(super) fn instruction(&mut self, pc: u32, inst: &NativeInst) {
        let at = Cursor::at(pc, self.position());
        let step = match inst {
            NativeInst::Unit(test) => m::unit(self, *test, at),
            NativeInst::UnitRepeat { test, min, max, greedy, atomic } => m::unit_repeat(self, (*test, *min, *max), (*greedy, *atomic), at),
            NativeInst::Assert(test) => m::assertion(self, *test, at),
            NativeInst::Split { alternative } => m::split(self, *alternative, at),
            NativeInst::GuardedSplit { alternative, guard } => m::guarded_split(self, *alternative, *guard, at),
            NativeInst::Jump(target) => Step::Continue(at.jump(*target)),
            NativeInst::Match => Step::Accept,
            NativeInst::Look { .. } => return self.look(pc),
            NativeInst::LookSucceed => return self.look_succeed(pc),
            other => self.registers_step(other, at),
        };
        self.finish_step(step);
    }

    /// The instructions that read and write registers: captures and the loops' counters.
    fn registers_step(&mut self, inst: &NativeInst, at: Cursor<ir::Value>) -> Step<ir::Value> {
        match inst {
            NativeInst::CaptureOpen { temporary, count, restores } => m::capture_open(self, (*temporary, *count, *restores), at),
            NativeInst::CaptureClose { capture, temporary } => m::capture_close(self, (*capture, *temporary), at),
            NativeInst::ResetCaptures { captures } => {
                for capture in captures {
                    m::reset_capture(self, *capture);
                }
                Step::Continue(at.next())
            }
            NativeInst::LoopInit { counter } => m::loop_init(self, *counter, at),
            NativeInst::LoopHead { counter, min, max, greedy, exit } => m::loop_head(self, (*counter, *min, *max), (*greedy, *exit), at),
            NativeInst::LoopIterate { start } => m::loop_iterate(self, *start, at),
            NativeInst::EmptyCheck { start } => m::empty_check(self, *start, at),
            NativeInst::LoopTail { counter, start, min, head } => m::loop_tail(self, (*counter, *start, *min, *head), at),
            _ => unreachable!("`instruction` handles the others"),
        }
    }

    /// Ends the current block where `step` goes.
    fn finish_step(&mut self, step: Step<ir::Value>) {
        match step {
            Step::Continue(next) => {
                self.set_position(next.position);
                self.goto(next.pc);
            }
            Step::Fail => self.fail(),
            Step::Accept => {
                let position = self.position();
                self.b.leave(&[position]);
            }
            Step::Branched => {}
        }
    }
}

impl Machine for Emit<'_, '_> {
    fn constant(&mut self, value: usize) -> ir::Value {
        Emit::constant(self, value as i64)
    }

    fn unset(&mut self) -> ir::Value {
        Emit::constant(self, self.plan.unset as i64)
    }

    fn load(&mut self, register: u32) -> ir::Value {
        self.register(register)
    }

    fn store(&mut self, register: u32, value: ir::Value) {
        self.store_register(register, value);
    }

    fn taken_end(&mut self, test: NativeTest, from: ir::Value, to: ir::Value) -> ir::Value {
        self.scan(test, from, to)
    }

    fn ceiling(&mut self, position: ir::Value, count: u32) -> ir::Value {
        if count == UNBOUNDED {
            return self.length;
        }
        let reach = self.b.raw_binary_imm(Binary::Add, position, i64::from(count));
        let within = self.compare(Relation::UnsignedLess, reach, self.length);
        self.b.raw_select(within, reach, self.length)
    }

    fn push_restore(&mut self, register: u32, old: ir::Value) {
        let id = Emit::constant(self, RESTORE);
        let register = Emit::constant(self, i64::from(register));
        self.push_frame(id, register, old);
    }

    fn push_capture_restore(&mut self, capture: u32, start: ir::Value, end: ir::Value) {
        self.push_restore(CAPTURE_REGISTERS * capture, start);
        self.push_restore(CAPTURE_REGISTERS * capture + 1, end);
    }

    fn push_choice(&mut self, pc: u32, position: ir::Value) {
        self.push_resume(Resume::Choice(pc), position, position);
    }

    /// A repeat's frame names the repeat, whose resume continues after it.
    fn push_greedy(&mut self, pc: u32, floor: ir::Value, position: ir::Value) {
        self.push_resume(Resume::Greedy(pc - 1), floor, position);
    }

    fn push_lazy(&mut self, pc: u32, _: NativeTest, ceiling: ir::Value, position: ir::Value) {
        self.push_resume(Resume::Lazy(pc - 1), ceiling, position);
    }

    fn require(&mut self, flag: ir::Value) -> bool {
        let held = self.b.new_block();
        self.branch(flag, held, self.backtrack);
        self.b.enter_open(held);
        true
    }

    fn when(&mut self, flag: ir::Value, effect: impl FnOnce(&mut Self)) {
        let (run, done) = (self.b.new_block(), self.b.new_block());
        self.branch(flag, run, done);
        self.b.enter_open(run);
        effect(self);
        self.b.goto(done, &[]);
        self.b.enter_open(done);
    }

    fn choose(&mut self, flag: ir::Value, then: impl FnOnce(&mut Self) -> Step<ir::Value>, otherwise: impl FnOnce(&mut Self) -> Step<ir::Value>) -> Step<ir::Value> {
        let (first, second) = (self.b.new_block(), self.b.new_block());
        self.branch(flag, first, second);
        self.b.enter_open(first);
        let step = then(self);
        self.finish_step(step);
        self.b.enter_open(second);
        let step = otherwise(self);
        self.finish_step(step);
        Step::Branched
    }
}
