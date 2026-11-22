//! Strength reduction of a counter's product: `counter * scale` where the counter only steps by one
//! in its loop is the word `counter * scale` held on the loop's entering edge and advanced by
//! `scale` at each step, which replaces the multiplication in the body (`Lowering::hoisted_read`).
//! The word is exact where the product is: a versioned access's entry proves the product within
//! `IntRange::EXACT` at both ends of the counter's reach (`Lowering::check_reach`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::*;
use super::Induction;

impl Lowering<'_, '_> {
    /// The products whose loop is the one at `header`, held from the counter's value on its entering
    /// edge from `state`.
    pub(crate) fn start_inductions(&mut self, state: &mut State, header: usize) {
        let inductions: Vec<Induction> = self.ranges.inductions().iter().copied().filter(|induction| induction.span.0 == header).collect();
        if inductions.is_empty() { return; }
        let exit = self.guard_exit(state, header as u32);
        for induction in inductions {
            let counter = self.entry_register(state, induction.counter, exit);
            let step = self.entry_term(state, induction.scale, exit);
            let start = self.raw_binary(Binary::Mul, counter, step);
            let (value, advance) = (self.builder.declare_var(types::I64), self.builder.declare_var(types::I64));
            self.builder.def_var(value, start);
            self.builder.def_var(advance, step);
            let range = self.ranges.index_range(induction.multiply).expect("an induction's product has an index range");
            self.hoisted.insert(induction.multiply, LoopRead { value, ty: Type::int(super::super::index_math::held(range)),
                representation: Representation::Int64, receiver: None, header: None, step: Some(advance) });
        }
    }

    /// Advances each product held for the counter the instruction at `pc` steps: before the step, which no
    /// instruction between observes.
    pub(crate) fn advance_inductions(&mut self, pc: usize) {
        let instruction = self.unit.instructions[pc];
        for index in 0..self.ranges.inductions().len() {
            let induction = self.ranges.inductions()[index];
            let steps = (induction.span.0..=induction.span.1).contains(&pc)
                && instruction.written_registers().any(|written| written == induction.counter);
            let Some(LoopRead { value, step: Some(step), .. }) = self.hoisted.get(&induction.multiply).copied().filter(|_| steps) else { continue };
            let (current, step) = (self.builder.use_var(value), self.builder.use_var(step));
            let next = self.raw_binary(Binary::Add, current, step);
            self.builder.def_var(value, next);
        }
    }
}
