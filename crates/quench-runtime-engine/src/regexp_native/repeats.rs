//! Unit repeats, loops and the resume blocks backtracking jumps to.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{Emit, RegexpMachine, Resume, FIRST_OPERAND, SECOND_OPERAND};
use quench_regexp::backtrack::machine as m;
use quench_regexp::backtrack::matcher::MatchStage;
use quench_runtime::staged::machine::{Binary, Lane, Relation};
use quench_regexp::backtrack::{Inst, UnitTest};

impl<M: RegexpMachine> Emit<'_, M> {
    /// The first position in `from..to` whose unit fails `test`, or `to`.
    pub(crate) fn scan(&mut self, test: UnitTest, from: M::Word, to: M::Word) -> M::Word {
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

    /// The start of the run of units from `from` back to at least `to` that pass `test`.
    pub(crate) fn scan_down(&mut self, test: UnitTest, from: M::Word, to: M::Word) -> M::Word {
        let (head, body, exit) = (self.b.new_block(), self.b.new_block(), self.b.new_block());
        let index = self.b.block_parameter(head, Lane::I64);
        let start = self.b.block_parameter(exit, Lane::I64);
        self.b.goto(head, &[from]);
        self.b.enter_open(head);
        let above = self.compare(Relation::UnsignedGreater, index, to);
        self.b.branch_if(above, (body, &[]), (exit, &[index]));
        self.b.enter_open(body);
        let before = self.b.raw_binary_imm(Binary::Add, index, -1);
        let unit = self.unit_at(before);
        let passes = self.passes(unit, test);
        self.b.branch_if(passes, (head, &[before]), (exit, &[index]));
        self.b.enter_open(exit);
        start
    }

    /// The last unit in `floor..end` that passes `test`, branching to the backtrack block
    /// where none does.
    pub(crate) fn scan_back(&mut self, test: UnitTest, floor: M::Word, end: M::Word) -> M::Word {
        let (look, lower, found) = (self.b.new_block(), self.b.new_block(), self.b.new_block());
        let at = self.b.block_parameter(look, Lane::I64);
        let last = self.b.raw_binary_imm(Binary::Add, end, -1);
        self.b.goto(look, &[last]);
        self.b.enter_open(look);
        let unit = self.unit_at(at);
        let passes = self.passes(unit, test);
        let more = self.b.new_block();
        self.branch(passes, found, more);
        self.b.enter_open(more);
        let above = self.compare(Relation::UnsignedGreater, at, floor);
        self.branch(above, lower, self.backtrack);
        self.b.enter_open(lower);
        let below = self.b.raw_binary_imm(Binary::Add, at, -1);
        self.b.goto(look, &[below]);
        self.b.enter_open(found);
        at
    }

    /// The code a resume frame continues at: `sp` is the popped frame's address.
    pub(crate) fn resume(&mut self, resume: Resume) {
        let frame = self.top();
        let flags = self.flags;
        let first = self.b.raw_load(Lane::I64, flags, frame, FIRST_OPERAND);
        let second = self.b.raw_load(Lane::I64, flags, frame, SECOND_OPERAND);
        match resume {
            Resume::Choice(target) => {
                self.set_position(second);
                self.goto(target);
            }
            Resume::Greedy(pc) => {
                let follower = match self.plan.insts.get(pc as usize + 1) {
                    Some(Inst::Unit(test)) => Some(*test),
                    _ => None,
                };
                let step = m::give_back(self, pc + 1, follower, (first, second));
                self.finish_step(step);
            }
            Resume::Lazy(pc) => {
                let Inst::UnitRepeat { test, .. } = self.plan.insts[pc as usize] else { unreachable!("a lazy frame names a repeat") };
                let step = m::take_more(self, pc + 1, test, (first, second));
                self.finish_step(step);
            }
            Resume::GreedyBack(pc) => {
                let step = m::give_back_back(self, pc + 1, (first, second));
                self.finish_step(step);
            }
            Resume::LazyBack(pc) => {
                let Inst::UnitRepeatBack { test, .. } = self.plan.insts[pc as usize] else { unreachable!("a backward lazy frame names a backward repeat") };
                let step = m::take_more_back(self, pc + 1, test, (first, second));
                self.finish_step(step);
            }
            Resume::LookFail(pc) => self.look_failed(pc, first),
        }
    }
}
