//! Unit repeats, loops and the resume blocks backtracking jumps to.
use super::{Emit, Resume, FIRST_OPERAND, SECOND_OPERAND};
use quench_regexp::backtrack::matcher::MatchStage;
use crate::staged::machine::{Binary, Lane, MachineAccess, MachineFlow, MachineScalar, Relation};
use cranelift_codegen::ir::{self, MemFlagsData};
use quench_regexp::backtrack::native::{NativeInst, NativeTest, NATIVE_UNBOUNDED as UNBOUNDED};

impl Emit<'_, '_> {
    /// The first position in `from..to` whose unit fails `test`, or `to`.
    fn scan(&mut self, test: NativeTest, from: ir::Value, to: ir::Value) -> ir::Value {
        let (head, body, exit) = (self.b.new_block(), self.b.new_block(), self.b.new_block());
        let index = self.b.block_parameter(head, Lane::I64);
        let end = self.b.block_parameter(exit, Lane::I64);
        self.b.goto(head, &[from]);
        self.b.enter_open(head);
        let inside = self.compare(Relation::UnsignedLess, index, to);
        self.b.branch_if(inside, (body, &[]), (exit, &[index]));
        self.b.enter_open(body);
        let unit = self.unit_at(index);
        let passes = self.passes(unit, test);
        let next = self.b.raw_binary_imm(Binary::Add, index, 1);
        self.b.branch_if(passes, (head, &[next]), (exit, &[index]));
        self.b.enter_open(exit);
        end
    }

    /// Pushes `resume` with its two operands where `condition` holds.
    fn push_resume_if(&mut self, condition: ir::Value, resume: Resume, first: ir::Value, second: ir::Value) {
        let (push, done) = (self.b.new_block(), self.b.new_block());
        self.branch(condition, push, done);
        self.b.enter_open(push);
        self.push_resume(resume, first, second);
        self.b.goto(done, &[]);
        self.b.enter_open(done);
    }

    pub(super) fn unit_repeat(&mut self, pc: u32, test: NativeTest, (min, max): (u32, u32), greedy: bool) {
        let position = self.position();
        let ceiling = if max == UNBOUNDED {
            self.length
        } else {
            let reach = self.b.raw_binary_imm(Binary::Add, position, i64::from(max));
            let within = self.compare(Relation::UnsignedLess, reach, self.length);
            self.b.raw_select(within, reach, self.length)
        };
        let floor = self.b.raw_binary_imm(Binary::Add, position, i64::from(min));
        let fits = self.compare(Relation::UnsignedLessEqual, floor, self.length);
        self.require(fits);
        let (end, more, resume) = if greedy {
            (self.scan(test, position, ceiling), Relation::UnsignedGreater, (Resume::Greedy(pc), floor))
        } else {
            (self.scan(test, position, floor), Relation::UnsignedLess, (Resume::Lazy(pc), ceiling))
        };
        let enough = self.compare(Relation::UnsignedGreaterEqual, end, floor);
        self.require(enough);
        let bound = if greedy { floor } else { ceiling };
        let more = self.compare(more, end, bound);
        self.push_resume_if(more, resume.0, resume.1, end);
        self.set_position(end);
        self.next(pc);
    }

    /// The loop instructions: counters, iteration starts and the empty-iteration checks.
    pub(super) fn loop_instruction(&mut self, pc: u32, inst: &NativeInst) {
        match inst {
            NativeInst::LoopInit { counter } => {
                let zero = self.constant(0);
                self.set_register(*counter, zero);
                self.next(pc);
            }
            NativeInst::LoopHead { counter, min, max, greedy, exit } => self.loop_head(pc, (*counter, *min, *max), *greedy, *exit),
            NativeInst::LoopIterate { start } => {
                let position = self.position();
                self.set_register(*start, position);
                self.next(pc);
            }
            NativeInst::EmptyCheck { start } => {
                let position = self.position();
                let began = self.register(*start);
                let moved = self.compare(Relation::NotEqual, position, began);
                self.require(moved);
                self.next(pc);
            }
            NativeInst::LoopTail { counter, start, min, head } => self.loop_tail(*counter, *start, *min, *head),
            _ => unreachable!("a plan holds no other instruction here"),
        }
    }

    fn loop_head(&mut self, pc: u32, (counter, min, max): (u32, u32, u32), greedy: bool, exit: u32) {
        let count = self.register(counter);
        let (enter, decide) = (self.b.new_block(), self.b.new_block());
        let below = self.compare_constant(Relation::UnsignedLess, count, i64::from(min));
        self.branch(below, enter, decide);
        self.b.enter_open(enter);
        self.next(pc);
        self.b.enter_open(decide);
        if max != UNBOUNDED {
            let (leave, choose) = (self.b.new_block(), self.b.new_block());
            let full = self.compare_constant(Relation::UnsignedGreaterEqual, count, i64::from(max));
            self.branch(full, leave, choose);
            self.b.enter_open(leave);
            self.goto(exit);
            self.b.enter_open(choose);
        }
        let position = self.position();
        let (kept, taken) = if greedy { (exit, pc + 1) } else { (pc + 1, exit) };
        self.push_resume(Resume::Choice(kept), position, position);
        self.goto(taken);
    }

    fn loop_tail(&mut self, counter: u32, start: u32, min: u32, head: u32) {
        let count = self.register(counter);
        let enough = self.compare_constant(Relation::UnsignedGreaterEqual, count, i64::from(min));
        let position = self.position();
        let began = self.register(start);
        let empty = self.compare(Relation::Equal, position, began);
        let rejected = self.b.raw_binary(Binary::And, enough, empty);
        let (reject, continues) = (self.b.new_block(), self.b.new_block());
        self.branch(rejected, reject, continues);
        self.b.enter_open(reject);
        self.fail();
        self.b.enter_open(continues);
        let next = self.b.raw_binary_imm(Binary::Add, count, 1);
        self.set_register(counter, next);
        self.goto(head);
    }

    /// The code a resume frame continues at: `sp` is the popped frame's address.
    pub(super) fn resume(&mut self, resume: Resume) {
        let frame = self.top();
        let flags = MemFlagsData::trusted();
        let first = self.b.raw_load(Lane::I64, flags, frame, FIRST_OPERAND);
        let second = self.b.raw_load(Lane::I64, flags, frame, SECOND_OPERAND);
        match resume {
            Resume::Choice(target) => {
                self.set_position(second);
                self.goto(target);
            }
            Resume::Greedy(pc) => self.greedy_resume(pc, first, second),
            Resume::Lazy(pc) => self.lazy_resume(pc, first, second),
        }
    }

    /// A greedy repeat gives back a unit: the frame goes back down to the floor.
    fn greedy_resume(&mut self, pc: u32, floor: ir::Value, end: ir::Value) {
        let at = self.b.raw_binary_imm(Binary::Add, end, -1);
        let more = self.compare(Relation::UnsignedGreater, at, floor);
        self.push_resume_if(more, Resume::Greedy(pc), floor, at);
        self.set_position(at);
        self.next(pc);
    }

    /// A lazy repeat takes a unit where its test passes, and keeps the frame below the ceiling.
    fn lazy_resume(&mut self, pc: u32, ceiling: ir::Value, at: ir::Value) {
        let NativeInst::UnitRepeat { test, .. } = self.plan.insts[pc as usize] else { unreachable!("a lazy frame names a repeat") };
        let unit = self.unit_at(at);
        let passes = self.passes(unit, test);
        self.require(passes);
        let next = self.b.raw_binary_imm(Binary::Add, at, 1);
        let more = self.compare(Relation::UnsignedLess, next, ceiling);
        self.push_resume_if(more, Resume::Lazy(pc), ceiling, next);
        self.set_position(next);
        self.next(pc);
    }
}
