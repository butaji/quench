//! Unit repeats, loops and the resume blocks backtracking jumps to.
use super::{Emit, Resume, FIRST_OPERAND, SECOND_OPERAND};
use quench_regexp::backtrack::machine::Machine;
use quench_regexp::backtrack::matcher::MatchStage;
use crate::staged::machine::{Binary, Lane, MachineAccess, MachineFlow, MachineScalar, Relation};
use cranelift_codegen::ir::{self, MemFlagsData};
/// The instruction after a repeat's one-unit follower, counted from the repeat.
const FOLLOWER_NEXT: u32 = 2;
use quench_regexp::backtrack::native::{NativeInst, NativeTest};

impl Emit<'_, '_> {
    /// The first position in `from..to` whose unit fails `test`, or `to`.
    pub(super) fn scan(&mut self, test: NativeTest, from: ir::Value, to: ir::Value) -> ir::Value {
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
            Resume::LookFail(pc) => self.look_failed(pc, first),
        }
    }

    /// A greedy repeat gives back a unit: the frame goes back down to the floor.
    fn greedy_resume(&mut self, pc: u32, floor: ir::Value, end: ir::Value) {
        if let Some(NativeInst::Unit(follower)) = self.plan.insts.get(pc as usize + 1) {
            return self.greedy_resume_to(pc, *follower, floor, end);
        }
        let at = self.b.raw_binary_imm(Binary::Add, end, -1);
        let more = self.compare(Relation::UnsignedGreater, at, floor);
        self.push_resume_if(more, Resume::Greedy(pc), floor, at);
        self.set_position(at);
        self.next(pc);
    }

    /// `greedy_resume` where the repeat's next instruction takes one unit by `follower`: the
    /// units given back that `follower` refuses fail at once, so one loop gives back down to
    /// the last unit it takes, which it takes, as a resume per unit followed by the `Unit`
    /// would, and fails where none down to the floor does.
    fn greedy_resume_to(&mut self, pc: u32, follower: NativeTest, floor: ir::Value, end: ir::Value) {
        let (look, step, found) = (self.b.new_block(), self.b.new_block(), self.b.new_block());
        let at = self.b.block_parameter(look, Lane::I64);
        let first = self.b.raw_binary_imm(Binary::Add, end, -1);
        self.b.goto(look, &[first]);
        self.b.enter_open(look);
        let unit = self.unit_at(at);
        let takes = self.passes(unit, follower);
        self.branch(takes, found, step);
        self.b.enter_open(step);
        let more = self.compare(Relation::UnsignedGreater, at, floor);
        let lower = self.b.new_block();
        self.branch(more, lower, self.backtrack);
        self.b.enter_open(lower);
        let below = self.b.raw_binary_imm(Binary::Add, at, -1);
        self.b.goto(look, &[below]);
        self.b.enter_open(found);
        let more = self.compare(Relation::UnsignedGreater, at, floor);
        self.push_resume_if(more, Resume::Greedy(pc), floor, at);
        let after = self.b.raw_binary_imm(Binary::Add, at, 1);
        self.set_position(after);
        // The follower took the unit: on past it.
        self.goto(pc + FOLLOWER_NEXT);
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
