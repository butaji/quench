//! `Machine` on the emitter: the VM's instructions are `quench_regexp::backtrack::machine`'s
//! functions, the interpreter's own, run here with the program's operands static, so each
//! instruction's code is derived from the semantics rather than written beside it. A flag the
//! machine requires branches to the backtrack block; a choice branches and finishes both paths.
use super::{Emit, RegexpMachine, Resume, RESTORE};
use crate::staged::machine::{Binary, Relation};
use quench_regexp::backtrack::native::NATIVE_ASCII_UNITS;
use quench_regexp::backtrack::machine::{self as m, Cursor, Machine, Step, CAPTURE_REGISTERS, UNBOUNDED};
use quench_regexp::backtrack::matcher::{Admission, MatchStage};
use quench_regexp::backtrack::native::{native_class_ends, CASE_BIT, CLASS_COUNT_SHIFT, CLASS_NONE, CLASS_OVERFLOW};
use quench_regexp::backtrack::{Inst, UnitTest};
use crate::staged::machine::Lane;
use super::FRAME_BYTES;

impl<M: RegexpMachine> Emit<'_, M> {
    /// The code of the instruction at `pc`: its row, specialized to it.
    pub(super) fn instruction(&mut self, pc: u32, inst: Inst) {
        let at = Cursor::at(pc, self.position());
        let step = m::step(self, inst, at);
        self.finish_step(step);
    }

    /// Ends the current block where `step` goes.
    pub(super) fn finish_step(&mut self, step: Step<M::Word>) {
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

impl<M: RegexpMachine> Machine for Emit<'_, M> {
    fn constant(&mut self, value: usize) -> M::Word {
        Emit::constant(self, value as i64)
    }

    fn unset(&mut self) -> M::Word {
        Emit::constant(self, self.plan.unset as i64)
    }

    fn load(&mut self, register: u32) -> M::Word {
        self.register(register)
    }

    fn store(&mut self, register: u32, value: M::Word) {
        self.store_register(register, value);
    }

    fn taken_end(&mut self, test: UnitTest, from: M::Word, to: M::Word) -> M::Word {
        self.scan(test, from, to)
    }

    fn ceiling(&mut self, position: M::Word, count: u32) -> M::Word {
        if count == UNBOUNDED {
            return self.length;
        }
        let reach = self.b.raw_binary_imm(Binary::Add, position, i64::from(count));
        let within = self.compare(Relation::UnsignedLess, reach, self.length);
        self.b.raw_select(within, reach, self.length)
    }

    fn push_restore(&mut self, register: u32, old: M::Word) {
        let id = Emit::constant(self, RESTORE);
        let register = Emit::constant(self, i64::from(register));
        self.push_frame(id, register, old);
    }

    fn push_capture_restore(&mut self, capture: u32, start: M::Word, end: M::Word) {
        self.push_restore(CAPTURE_REGISTERS * capture, start);
        self.push_restore(CAPTURE_REGISTERS * capture + 1, end);
    }

    fn push_choice(&mut self, pc: u32, position: M::Word) {
        self.push_resume(Resume::Choice(pc), position, position);
    }

    /// A repeat's frame names the repeat, whose resume continues after it.
    fn push_greedy(&mut self, pc: u32, floor: M::Word, position: M::Word) {
        self.push_resume(Resume::Greedy(pc - 1), floor, position);
    }

    fn push_lazy(&mut self, pc: u32, _: UnitTest, ceiling: M::Word, position: M::Word) {
        self.push_resume(Resume::Lazy(pc - 1), ceiling, position);
    }

    fn last_passing(&mut self, test: UnitTest, floor: M::Word, end: M::Word) -> Option<M::Word> {
        Some(self.scan_back(test, floor, end))
    }

    fn keep_greedy(&mut self, pc: u32, floor: M::Word, position: M::Word) {
        self.push_greedy(pc, floor, position);
    }

    fn keep_lazy(&mut self, pc: u32, test: UnitTest, ceiling: M::Word, position: M::Word) {
        self.push_lazy(pc, test, ceiling, position);
    }

    fn listed(&mut self, index: u32) -> u32 {
        self.plan.lists[index as usize]
    }

    fn pick(&mut self, flag: M::Word, then: M::Word, otherwise: M::Word) -> M::Word {
        self.b.raw_select(flag, then, otherwise)
    }

    fn plus(&mut self, left: M::Word, right: M::Word) -> M::Word {
        self.b.raw_binary(Binary::Add, left, right)
    }

    fn minus(&mut self, left: M::Word, right: M::Word) -> M::Word {
        self.b.raw_binary(Binary::Sub, left, right)
    }

    fn reach_back(&mut self, position: M::Word, count: u32) -> M::Word {
        let zero = Emit::constant(self, 0);
        if count == UNBOUNDED {
            return zero;
        }
        let count = Emit::constant(self, i64::from(count));
        let within = self.compare(Relation::UnsignedLess, position, count);
        let reach = self.b.raw_binary(Binary::Sub, position, count);
        self.b.raw_select(within, zero, reach)
    }

    fn run_start(&mut self, test: UnitTest, from: M::Word, to: M::Word) -> M::Word {
        self.scan_down(test, from, to)
    }

    fn same_units(&mut self, (from, to): (M::Word, M::Word), at: M::Word, (ignore_case, unicode): (bool, bool)) -> M::Word {
        let (head, body, step, done) = (self.b.new_block(), self.b.new_block(), self.b.new_block(), self.b.new_block());
        let (expected, actual) = (self.b.block_parameter(head, Lane::I64), self.b.block_parameter(head, Lane::I64));
        let same = self.b.block_parameter(done, Lane::I8);
        let (held, failed) = (self.b.raw_const(Lane::I8, 1), self.b.raw_const(Lane::I8, 0));
        self.b.goto(head, &[from, at]);
        self.b.enter_open(head);
        let inside = self.compare(Relation::UnsignedLess, expected, to);
        self.b.branch_if(inside, (body, &[]), (done, &[held]));
        self.b.enter_open(body);
        let (left, right) = (self.unit_at(expected), self.unit_at(actual));
        let mut equal = self.compare(Relation::Equal, left, right);
        if ignore_case {
            // An ASCII unit and its case partner are one character where folding says so.
            let differ = self.b.raw_binary(Binary::Xor, left, right);
            let partners = self.compare_constant(Relation::Equal, differ, i64::from(CASE_BIT));
            let folds = self.in_mask(self.plan.folds[usize::from(unicode)], left);
            let folded = self.b.raw_binary(Binary::And, partners, folds);
            equal = self.b.raw_binary(Binary::Or, equal, folded);
        }
        self.b.branch_if(equal, (step, &[]), (done, &[failed]));
        self.b.enter_open(step);
        let (expected, actual) = (self.b.raw_binary_imm(Binary::Add, expected, 1), self.b.raw_binary_imm(Binary::Add, actual, 1));
        self.b.goto(head, &[expected, actual]);
        self.b.enter_open(done);
        same
    }

    fn push_greedy_back(&mut self, pc: u32, ceiling: M::Word, position: M::Word) {
        self.push_resume(Resume::GreedyBack(pc - 1), ceiling, position);
    }

    fn push_lazy_back(&mut self, pc: u32, _: UnitTest, floor: M::Word, position: M::Word) {
        self.push_resume(Resume::LazyBack(pc - 1), floor, position);
    }

    fn keep_greedy_back(&mut self, pc: u32, ceiling: M::Word, position: M::Word) {
        self.push_greedy_back(pc, ceiling, position);
    }

    fn keep_lazy_back(&mut self, pc: u32, test: UnitTest, floor: M::Word, position: M::Word) {
        self.push_lazy_back(pc, test, floor, position);
    }

    fn follower(&mut self, pc: u32) -> Option<UnitTest> {
        match self.plan.insts.get(pc as usize) {
            Some(Inst::Unit(test)) => Some(*test),
            _ => None,
        }
    }

    fn guard(&mut self, guard: u32) -> Admission<UnitTest> {
        self.plan.guards[guard as usize]
    }

    fn lookaround(&mut self, _: (bool, u32), at: Cursor<M::Word>) -> Step<M::Word> {
        self.look(at.pc);
        Step::Branched
    }

    fn look_succeed(&mut self, at: Cursor<M::Word>) -> Step<M::Word> {
        self.look_succeed(at.pc);
        Step::Branched
    }

    /// The table's intervals as a chain of range tests, each ending in its arm's code; an
    /// ASCII subject reaches only their ASCII part.
    fn dispatch_on(&mut self, unit: M::Word, table: u32, mut arm: impl FnMut(&mut Self, (u32, u32)) -> Step<M::Word>) -> Step<M::Word> {
        let plan = self.plan;
        for interval in plan.dispatches[table as usize].iter().filter(|interval| interval.lo < NATIVE_ASCII_UNITS) {
            let hit = self.in_range(unit, (interval.lo, interval.hi.min(NATIVE_ASCII_UNITS - 1)));
            let (taken, next) = (self.b.new_block(), self.b.new_block());
            self.branch(hit, taken, next);
            self.b.enter_open(taken);
            let step = arm(self, (interval.first, interval.count));
            self.finish_step(step);
            self.b.enter_open(next);
        }
        self.fail();
        Step::Branched
    }

    /// The class's matches come from `native_class_ends`, which pushes the shorter ones as
    /// choice frames itself.
    fn class_ends(&mut self, class: u32, position: M::Word, backward: bool, pc: u32) -> (M::Word, M::Word) {
        let choice = self.resume_id(Resume::Choice(pc));
        let top = self.top();
        let arguments = [
            Emit::constant(self, self.plan.classes as i64),
            Emit::constant(self, i64::from(class) << 1 | i64::from(backward)),
            self.bytes,
            self.length,
            position,
            top,
            self.stack_end,
            Emit::constant(self, choice),
        ];
        let ends = self.b.call_leaf(native_class_ends as *const () as usize, &arguments);
        let overflowed = self.compare_constant(Relation::Equal, ends, CLASS_OVERFLOW as i64);
        let fits = self.b.new_block();
        self.branch(overflowed, self.overflow, fits);
        self.b.enter_open(fits);
        let found = self.compare_constant(Relation::NotEqual, ends, CLASS_NONE as i64);
        let count = self.b.raw_binary_imm(Binary::ShiftRightUnsigned, ends, i64::from(CLASS_COUNT_SHIFT));
        let zero = Emit::constant(self, 0);
        let count = self.b.raw_select(found, count, zero);
        let pushed = self.b.raw_binary_imm(Binary::Mul, count, FRAME_BYTES);
        let top = self.b.raw_binary(Binary::Add, top, pushed);
        self.b.define_variable(self.top, top);
        let end = self.b.raw_binary_imm(Binary::And, ends, i64::from(u32::MAX));
        (found, end)
    }

    fn require(&mut self, flag: M::Word) -> bool {
        let held = self.b.new_block();
        self.branch(flag, held, self.backtrack);
        self.b.enter_open(held);
        true
    }

    fn when(&mut self, flag: M::Word, effect: impl FnOnce(&mut Self)) {
        let (run, done) = (self.b.new_block(), self.b.new_block());
        self.branch(flag, run, done);
        self.b.enter_open(run);
        effect(self);
        self.b.goto(done, &[]);
        self.b.enter_open(done);
    }

    fn choose(&mut self, flag: M::Word, then: impl FnOnce(&mut Self) -> Step<M::Word>, otherwise: impl FnOnce(&mut Self) -> Step<M::Word>) -> Step<M::Word> {
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
