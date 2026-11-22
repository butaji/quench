//! Lookaheads in native code, as `Concrete::look` runs them: the body runs above a mark on
//! the stack; reaching `LookSucceed` decides a positive lookahead (its choices go, the
//! restores of its captures stay) or fails a negative one (every entry above the mark is
//! undone), and a backtrack down to the mark decides the other way.
use super::{Emit, RegexpMachine, Resume, FRAME_BYTES, FIRST_OPERAND, RESTORE, SECOND_OPERAND};
use quench_runtime::staged::machine::{Binary, Lane, Relation};
use quench_regexp::backtrack::native::NativeInst;

impl<M: RegexpMachine> Emit<'_, M> {
    /// The variable holding the mark of the lookahead at `look`.
    fn mark(&mut self, look: u32) -> M::Variable {
        if let Some((_, mark)) = self.marks.iter().find(|(pc, _)| *pc == look) {
            return *mark;
        }
        let mark = self.b.declare_variable(Lane::I64);
        self.marks.push((look, mark));
        mark
    }

    /// The lookahead whose body ends at the `LookSucceed` at `pc`.
    fn look_of(&self, pc: u32) -> (u32, bool) {
        self.plan.insts.iter().enumerate().find_map(|(look, inst)| match inst {
            NativeInst::Look { negative, end } if *end == pc + 1 => Some((look as u32, *negative)),
            _ => None,
        }).expect("a LookSucceed ends a lookahead's body")
    }

    /// Pushes the lookahead's mark, with the position the match goes on from, and runs its body.
    pub(super) fn look(&mut self, pc: u32) {
        let mark = self.mark(pc);
        let top = self.top();
        self.b.define_variable(mark, top);
        let position = self.position();
        let zero = self.constant(0);
        self.push_resume(Resume::LookFail(pc), position, zero);
        self.next(pc);
    }

    /// The body matched.
    pub(super) fn look_succeed(&mut self, pc: u32) {
        let (look, negative) = self.look_of(pc);
        let mark = self.mark(look);
        let mark = self.b.read_variable(mark);
        let flags = self.flags;
        let position = self.b.raw_load(Lane::I64, flags, mark, FIRST_OPERAND);
        if negative {
            self.undo_to(mark);
            self.b.define_variable(self.top, mark);
            self.fail();
            return;
        }
        self.keep_restores(mark);
        self.set_position(position);
        let NativeInst::Look { end, .. } = self.plan.insts[look as usize] else { unreachable!("a lookahead") };
        self.goto(end);
    }

    /// The backtrack popped the mark: the body found no match.
    pub(super) fn look_failed(&mut self, pc: u32, position: M::Word) {
        let NativeInst::Look { negative, end } = self.plan.insts[pc as usize] else { unreachable!("a lookahead's mark") };
        if !negative {
            self.fail();
            return;
        }
        self.set_position(position);
        self.goto(end);
    }

    /// Pops and undoes every entry above `mark`, down to the mark itself.
    fn undo_to(&mut self, mark: M::Word) {
        let (pop, restore, done) = (self.b.new_block(), self.b.new_block(), self.b.new_block());
        let above = self.b.raw_binary_imm(Binary::Add, mark, FRAME_BYTES);
        self.b.goto(pop, &[]);
        self.b.enter_open(pop);
        let top = self.top();
        let empty = self.compare(Relation::Equal, top, above);
        let next = self.b.new_block();
        self.branch(empty, done, next);
        self.b.enter_open(next);
        let frame = self.b.raw_binary_imm(Binary::Add, top, -FRAME_BYTES);
        self.b.define_variable(self.top, frame);
        let flags = self.flags;
        let id = self.b.raw_load(Lane::I64, flags, frame, 0);
        let restores = self.compare_constant(Relation::Equal, id, RESTORE);
        self.branch(restores, restore, pop);
        self.b.enter_open(restore);
        let frame = self.top();
        self.write_back(frame);
        self.b.goto(pop, &[]);
        self.b.enter_open(done);
    }

    /// Moves the restore entries above `mark` down over it, in order, and drops the rest.
    fn keep_restores(&mut self, mark: M::Word) {
        let (scan, copy, step, done) = (self.b.new_block(), self.b.new_block(), self.b.new_block(), self.b.new_block());
        let (from, to) = (self.b.declare_variable(Lane::I64), self.b.declare_variable(Lane::I64));
        let first = self.b.raw_binary_imm(Binary::Add, mark, FRAME_BYTES);
        self.b.define_variable(from, first);
        self.b.define_variable(to, mark);
        self.b.goto(scan, &[]);
        self.b.enter_open(scan);
        let (source, top) = (self.b.read_variable(from), self.top());
        let finished = self.compare(Relation::Equal, source, top);
        let next = self.b.new_block();
        self.branch(finished, done, next);
        self.b.enter_open(next);
        let flags = self.flags;
        let id = self.b.raw_load(Lane::I64, flags, source, 0);
        let restores = self.compare_constant(Relation::Equal, id, RESTORE);
        self.branch(restores, copy, step);
        self.b.enter_open(copy);
        let (source, target) = (self.b.read_variable(from), self.b.read_variable(to));
        for offset in [0, FIRST_OPERAND, SECOND_OPERAND] {
            let word = self.b.raw_load(Lane::I64, flags, source, offset);
            self.b.raw_store(Lane::I64, flags, (word, false), target, offset);
        }
        let target = self.b.raw_binary_imm(Binary::Add, target, FRAME_BYTES);
        self.b.define_variable(to, target);
        self.b.goto(step, &[]);
        self.b.enter_open(step);
        let source = self.b.read_variable(from);
        let source = self.b.raw_binary_imm(Binary::Add, source, FRAME_BYTES);
        self.b.define_variable(from, source);
        self.b.goto(scan, &[]);
        self.b.enter_open(done);
        let kept = self.b.read_variable(to);
        self.b.define_variable(self.top, kept);
    }
}
