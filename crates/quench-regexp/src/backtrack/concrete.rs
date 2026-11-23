//! `Stage` on the interpreter's machine: every operation on its own state, inlined into
//! `Program::run`.
#[allow(clippy::wildcard_imports)]
use super::*;
use crate::backtrack::exhaustion::reserve_frames;

/// The interpreter's machine: the program, the subject and the run's registers and stack.
pub(in crate::backtrack) struct Concrete<'a, T: Text + ?Sized> {
    pub(in crate::backtrack) program: &'a Program,
    pub(in crate::backtrack) text: &'a T,
    pub(in crate::backtrack) registers: &'a mut [usize],
    pub(in crate::backtrack) stack: &'a mut Vec<Frame>,
}

quench_stack::projection!([T: Text + ?Sized] Concrete<'_, T> => Concrete);

impl<T: Text + ?Sized> MatchStage for Concrete<'_, T> {
    type Pos = usize;
    type Unit = u32;
    type Flag = bool;
    type Test = UnitTest;

    #[inline(always)]
    fn length(&mut self) -> usize {
        self.text.len()
    }

    #[inline(always)]
    fn at_start(&mut self, position: usize) -> bool {
        position == 0
    }

    #[inline(always)]
    fn equal(&mut self, left: usize, right: usize) -> bool {
        left == right
    }

    #[inline(always)]
    fn before(&mut self, left: usize, right: usize) -> bool {
        left < right
    }

    #[inline(always)]
    fn offset(&mut self, position: usize, delta: isize) -> usize {
        position.wrapping_add_signed(delta)
    }

    #[inline(always)]
    fn unit_at(&mut self, index: usize) -> u32 {
        self.text.unit(index)
    }

    #[inline(always)]
    fn passes(&mut self, unit: u32, test: UnitTest) -> bool {
        self.program.test(test, unit)
    }

    #[inline(always)]
    fn in_ranges(&mut self, unit: u32, ranges: &[(u32, u32)]) -> bool {
        in_units(unit, ranges)
    }

    #[inline(always)]
    fn not(&mut self, flag: bool) -> bool {
        !flag
    }

    #[inline(always)]
    fn differ(&mut self, left: bool, right: bool) -> bool {
        left != right
    }

    #[inline(always)]
    fn guarded(&mut self, condition: bool, then: impl FnOnce(&mut Self) -> bool) -> bool {
        condition && then(self)
    }

    #[inline(always)]
    fn or_else(&mut self, first: bool, otherwise: impl FnOnce(&mut Self) -> bool) -> bool {
        first || otherwise(self)
    }
}

impl<T: Text + ?Sized> Machine for Concrete<'_, T> {
    #[inline(always)]
    fn constant(&mut self, value: usize) -> usize {
        value
    }

    #[inline(always)]
    fn unset(&mut self) -> usize {
        UNSET
    }

    #[inline(always)]
    fn load(&mut self, register: u32) -> usize {
        self.registers[register as usize]
    }

    #[inline(always)]
    fn store(&mut self, register: u32, value: usize) {
        self.registers[register as usize] = value;
    }

    #[inline(always)]
    fn taken_end(&mut self, test: UnitTest, from: usize, to: usize) -> usize {
        self.program.run_end(test, self.text, from, to)
    }

    #[inline(always)]
    fn ceiling(&mut self, position: usize, count: u32) -> usize {
        position.saturating_add(count as usize).min(self.text.len())
    }

    #[inline(always)]
    fn push_restore(&mut self, register: u32, value: usize) {
        self.push(Frame::Restore { register, value });
    }

    #[inline(always)]
    fn push_capture_restore(&mut self, capture: u32, start: usize, end: usize) {
        self.push(Frame::RestoreCapture { capture, start, end });
    }

    #[inline(always)]
    fn push_choice(&mut self, pc: u32, position: usize) {
        self.push(Frame::Choice { pc, position });
    }

    #[inline(always)]
    fn push_greedy(&mut self, pc: u32, floor: usize, position: usize) {
        self.push(Frame::Greedy { pc, floor, position });
    }

    #[inline(always)]
    fn push_lazy(&mut self, pc: u32, test: UnitTest, ceiling: usize, position: usize) {
        self.push(Frame::Lazy { pc, test, ceiling, position });
    }

    #[inline(always)]
    fn last_passing(&mut self, test: UnitTest, floor: usize, end: usize) -> Option<usize> {
        let mut at = end - 1;
        while !self.program.test(test, self.text.unit(at)) {
            if at == floor {
                return None;
            }
            at -= 1;
        }
        Some(at)
    }

    #[inline(always)]
    fn keep_greedy(&mut self, pc: u32, floor: usize, position: usize) {
        self.keep(Frame::Greedy { pc, floor, position });
    }

    #[inline(always)]
    fn keep_lazy(&mut self, pc: u32, test: UnitTest, ceiling: usize, position: usize) {
        self.keep(Frame::Lazy { pc, test, ceiling, position });
    }

    #[inline(always)]
    fn require(&mut self, flag: bool) -> bool {
        flag
    }

    #[inline(always)]
    fn when(&mut self, flag: bool, effect: impl FnOnce(&mut Self)) {
        if flag {
            effect(self);
        }
    }

    #[inline(always)]
    fn choose(&mut self, flag: bool, then: impl FnOnce(&mut Self) -> Step, otherwise: impl FnOnce(&mut Self) -> Step) -> Step {
        if flag { then(self) } else { otherwise(self) }
    }
}

impl<T: Text + ?Sized> Stage for Concrete<'_, T> {

    #[inline(always)]
    fn run_start(&self, test: UnitTest, from: usize, to: usize) -> usize {
        self.program.run_start_back(test, self.text, from, to)
    }

    #[inline(always)]
    fn register(&self, register: u32) -> usize {
        self.registers[register as usize]
    }

    #[inline(always)]
    fn assign(&mut self, register: u32, value: usize) {
        self.registers[register as usize] = value;
    }

    #[inline(always)]
    fn push(&mut self, frame: Frame) {
        let length = self.stack.len();
        if length == self.stack.capacity() && !reserve_frames(self.stack) {
            return;
        }
        // SAFETY: the check above leaves room for one more frame (`reserve_frames` cleared the
        // stack where it made none), and a `Frame` is `Copy`.
        unsafe {
            let length = self.stack.len();
            self.stack.as_mut_ptr().add(length).write(frame);
            self.stack.set_len(length + 1);
        }
    }

    #[inline(always)]
    fn keep(&mut self, frame: Frame) {
        let length = self.stack.len();
        debug_assert!(length < self.stack.capacity());
        // SAFETY: `keep` follows the `pop` that freed this slot, so it lies within the
        // capacity, and a `Frame` is `Copy`.
        unsafe {
            self.stack.as_mut_ptr().add(length).write(frame);
            self.stack.set_len(length + 1);
        }
    }

    #[inline(always)]
    fn pop(&mut self) -> Option<Frame> {
        self.stack.pop()
    }

    #[inline(always)]
    fn guard(&self, guard: u32) -> Admission<UnitTest> {
        self.program.guards[guard as usize]
    }

    #[inline(always)]
    fn dispatch(&mut self, table: u32, position: usize) -> Option<u32> {
        self.program.dispatch_target(table, self.text, position, self.stack)
    }

    fn class_ends(&mut self, class: u32, position: usize, backward: bool) -> Vec<usize> {
        self.program.classes[class as usize].ends(self.text, position, backward)
    }

    #[inline(always)]
    fn unit_follower(&self, pc: u32) -> Option<UnitTest> {
        match self.program.instructions[pc as usize] {
            Inst::Unit(test) => Some(test),
            _ => None,
        }
    }

    #[inline(always)]
    fn listed(&self, index: u32) -> u32 {
        self.program.lists[index as usize]
    }

    fn look(&mut self, body: Cursor, negative: bool) -> bool {
        let base = self.stack.len();
        self.push(Frame::Barrier);
        let found = self.program.run(self.text, (body.pc, body.position, body.position), None, self.registers, self.stack).is_some();
        if found && !negative {
            // A lookahead is atomic: its choices go, the restores of its captures stay for
            // backtracking past it.
            let mut kept = base;
            for index in base + 1..self.stack.len() {
                if let Frame::Restore { .. } | Frame::RestoreCapture { .. } = self.stack[index] {
                    self.stack[kept] = self.stack[index];
                    kept += 1;
                }
            }
            self.stack.truncate(kept);
        } else if found {
            while self.stack.len() > base {
                if let Some(frame) = self.stack.pop() {
                    undo(self, frame);
                }
            }
        }
        found != negative
    }
}
