//! Unit repeats, loops and the resume blocks backtracking jumps to.
use super::{Emit, Resume, FIRST_OPERAND, SECOND_OPERAND};
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlagsData};
use quench_regexp::backtrack::native::{NativeInst, NativeTest, NATIVE_UNBOUNDED as UNBOUNDED};

impl Emit<'_, '_> {
    /// The first position in `from..to` whose unit fails `test`, or `to`.
    fn scan(&mut self, test: NativeTest, from: ir::Value, to: ir::Value) -> ir::Value {
        let (head, body, exit) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
        self.b.append_block_param(head, types::I64);
        self.b.append_block_param(exit, types::I64);
        self.b.ins().jump(head, &[from.into()]);
        self.b.switch_to_block(head);
        let index = self.b.block_params(head)[0];
        let inside = self.compare(IntCC::UnsignedLessThan, index, to);
        self.b.ins().brif(inside, body, &[], exit, &[index.into()]);
        self.b.switch_to_block(body);
        let byte = self.byte_at(index);
        let passes = self.passes(test, byte);
        let next = self.b.ins().iadd_imm_s(index, 1);
        self.b.ins().brif(passes, head, &[next.into()], exit, &[index.into()]);
        self.b.switch_to_block(exit);
        self.b.block_params(exit)[0]
    }

    /// Pushes `resume` with its two operands where `condition` holds.
    fn push_resume_if(&mut self, condition: ir::Value, resume: Resume, first: ir::Value, second: ir::Value) {
        let (push, done) = (self.b.create_block(), self.b.create_block());
        self.branch(condition, push, done);
        self.b.switch_to_block(push);
        self.push_resume(resume, first, second);
        self.b.ins().jump(done, &[]);
        self.b.switch_to_block(done);
    }

    pub(super) fn unit_repeat(&mut self, pc: u32, test: NativeTest, (min, max): (u32, u32), greedy: bool) {
        let position = self.position();
        let ceiling = if max == UNBOUNDED {
            self.length
        } else {
            let reach = self.b.ins().iadd_imm_s(position, i64::from(max));
            self.b.ins().umin(reach, self.length)
        };
        let floor = self.b.ins().iadd_imm_s(position, i64::from(min));
        let fits = self.compare(IntCC::UnsignedLessThanOrEqual, floor, self.length);
        self.require(fits);
        let (end, more, resume) = if greedy {
            (self.scan(test, position, ceiling), IntCC::UnsignedGreaterThan, (Resume::Greedy(pc), floor))
        } else {
            (self.scan(test, position, floor), IntCC::UnsignedLessThan, (Resume::Lazy(pc), ceiling))
        };
        let enough = self.compare(IntCC::UnsignedGreaterThanOrEqual, end, floor);
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
                let moved = self.compare(IntCC::NotEqual, position, began);
                self.require(moved);
                self.next(pc);
            }
            NativeInst::LoopTail { counter, start, min, head } => self.loop_tail(*counter, *start, *min, *head),
            _ => unreachable!("a plan holds no other instruction here"),
        }
    }

    fn loop_head(&mut self, pc: u32, (counter, min, max): (u32, u32, u32), greedy: bool, exit: u32) {
        let count = self.register(counter);
        let (enter, decide) = (self.b.create_block(), self.b.create_block());
        let below = self.compare_constant(IntCC::UnsignedLessThan, count, i64::from(min));
        self.branch(below, enter, decide);
        self.b.switch_to_block(enter);
        self.next(pc);
        self.b.switch_to_block(decide);
        if max != UNBOUNDED {
            let (leave, choose) = (self.b.create_block(), self.b.create_block());
            let full = self.compare_constant(IntCC::UnsignedGreaterThanOrEqual, count, i64::from(max));
            self.branch(full, leave, choose);
            self.b.switch_to_block(leave);
            self.goto(exit);
            self.b.switch_to_block(choose);
        }
        let position = self.position();
        let (kept, taken) = if greedy { (exit, pc + 1) } else { (pc + 1, exit) };
        self.push_resume(Resume::Choice(kept), position, position);
        self.goto(taken);
    }

    fn loop_tail(&mut self, counter: u32, start: u32, min: u32, head: u32) {
        let count = self.register(counter);
        let enough = self.compare_constant(IntCC::UnsignedGreaterThanOrEqual, count, i64::from(min));
        let position = self.position();
        let began = self.register(start);
        let empty = self.compare(IntCC::Equal, position, began);
        let rejected = self.b.ins().band(enough, empty);
        let (reject, continues) = (self.b.create_block(), self.b.create_block());
        self.branch(rejected, reject, continues);
        self.b.switch_to_block(reject);
        self.fail();
        self.b.switch_to_block(continues);
        let next = self.b.ins().iadd_imm_s(count, 1);
        self.set_register(counter, next);
        self.goto(head);
    }

    /// The code a resume frame continues at: `sp` is the popped frame's address.
    pub(super) fn resume(&mut self, resume: Resume) {
        let frame = self.top();
        let flags = MemFlagsData::trusted();
        let first = self.b.ins().load(types::I64, flags, frame, FIRST_OPERAND);
        let second = self.b.ins().load(types::I64, flags, frame, SECOND_OPERAND);
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
        let at = self.b.ins().iadd_imm_s(end, -1);
        let more = self.compare(IntCC::UnsignedGreaterThan, at, floor);
        self.push_resume_if(more, Resume::Greedy(pc), floor, at);
        self.set_position(at);
        self.next(pc);
    }

    /// A lazy repeat takes a unit where its test passes, and keeps the frame below the ceiling.
    fn lazy_resume(&mut self, pc: u32, ceiling: ir::Value, at: ir::Value) {
        let NativeInst::UnitRepeat { test, .. } = self.plan.insts[pc as usize] else { unreachable!("a lazy frame names a repeat") };
        let byte = self.byte_at(at);
        let passes = self.passes(test, byte);
        self.require(passes);
        let next = self.b.ins().iadd_imm_s(at, 1);
        let more = self.compare(IntCC::UnsignedLessThan, next, ceiling);
        self.push_resume_if(more, Resume::Lazy(pc), ceiling, next);
        self.set_position(next);
        self.next(pc);
    }
}
