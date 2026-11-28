//! The affine reach of an element access (`Reach::Affine`): the index range the analysis proves for it
//! (`FunctionFacts::index_ceiling`), a floor and a ceiling each the min or max of up to `FORMS` linear
//! forms over registers the access's loop does not write. The loop's entry evaluates each form on its
//! entering edge and compares the floor forms with 0 and the ceiling forms with the array's length, so
//! the access in the body needs no bounds test. A failed comparison leaves at the entry, where the
//! interpreter runs the loop; there is no checked twin.

use super::super::*;
use super::super::scalar::parameter_register;
use super::plan::access_registers;
use super::{Nest, Reach, Resolved, Version};
use quench_runtime::bytecode::Function;
use quench_runtime::compile::aot_facts::{Affine, Base};

/// The most forms an end of an index range holds (the analysis's `MAX_TERMS`).
pub(crate) const FORMS: usize = 4;

/// The forms of one end: the first `count` slots hold them, the rest are `None`.
pub(crate) type Linears = [Option<Linear>; FORMS];

/// The most a form's coefficient may be. An int32 register times it stays within 2^51, so the two
/// terms of a form and its offset stay within `i64` (`OFFSET_LIMIT`), and the entry's arithmetic is exact.
const COEFFICIENT_LIMIT: i64 = 1 << 20;

/// The most a form's offset may be.
const OFFSET_LIMIT: i64 = 1 << 40;

/// A linear form over registers, `offset + sum(coefficient * register)`, which the loop's entry reads
/// on its entering edge (each register an int32 there, else the entry leaves).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Linear {
    pub(crate) offset: i64,
    pub(crate) terms: [Option<(Register, i64)>; 2],
}

impl Linear {
    /// `form` as a linear form over registers, where its coefficients and offset are within the limits. A base
    /// that is a parameter reads its register by `parameter` (its entry value, which nothing in the function writes).
    fn of(form: Affine, parameter: &impl Fn(u16) -> Option<Register>) -> Option<Self> {
        if !(-OFFSET_LIMIT..=OFFSET_LIMIT).contains(&form.offset) { return None; }
        let mut terms = [None; 2];
        for (slot, term) in terms.iter_mut().zip(form.terms) {
            *slot = match term {
                None => None,
                Some((base, coefficient)) if (-COEFFICIENT_LIMIT..=COEFFICIENT_LIMIT).contains(&coefficient) => Some((register_of(base, parameter)?, coefficient)),
                Some(_) => return None,
            };
        }
        Some(Self { offset: form.offset, terms })
    }
}

/// The register a base's value is read from on a loop's entering edge: a register, or a parameter's entry register.
fn register_of(base: Base, parameter: &impl Fn(u16) -> Option<Register>) -> Option<Register> {
    match base {
        Base::Register(register) => Some(register),
        Base::Parameter(index) => parameter(index),
    }
}

/// The register parameter `index` enters in, where it is the register the analysis proves unwritten
/// (`AotFacts`' `promoted_register`): only then does the lane's parameter register hold the entry value a form reads.
fn parameter_entry_register(function: &Function, index: u16) -> Option<Register> {
    let register = parameter_register(function, usize::from(index))?;
    (function.promoted_register(index) == Some(register)).then_some(register)
}

/// The index range of an access as the lane takes it: the function PC of the loop it is invariant in, the forms of
/// its floor and ceiling, and whether the index is an integer wherever it is not NaN (`IndexRange::integral`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ends {
    pub(crate) header: u32,
    pub(crate) floor: Linears,
    pub(crate) ceiling: Linears,
    pub(crate) integral: bool,
}

/// The forms of one end, where each is admitted by `Linear::of`, there are at most `FORMS`, and there is at least one.
fn linears_of(end: impl Iterator<Item = Affine>, parameter: &impl Fn(u16) -> Option<Register>) -> Option<Linears> {
    let mut linears: Linears = [None; FORMS];
    let mut count = 0;
    for form in end {
        *linears.get_mut(count)? = Some(Linear::of(form, parameter)?);
        count += 1;
    }
    (count > 0).then_some(linears)
}

impl Lowering<'_, '_> {
    /// The index range of the element access at unit PC `pc`, where the solve proves one and its index is not NaN
    /// (`FunctionFacts::index_ceiling`). Only the caller's own accesses have one here: an inlined callee's
    /// registers and PCs are offset from the caller's, which this lowering does not map.
    pub(crate) fn index_ends(&self, pc: usize) -> Option<Ends> {
        let function_pc = self.caller_pc(pc)?;
        let range = self.vm.facts?.function_facts(self.function_id).index_ceiling(function_pc)?;
        if range.nan { return None; }
        let function = self.program.function_get(self.function_id as usize)?;
        let parameter = |index: u16| parameter_entry_register(function, index);
        Some(Ends { header: range.header, integral: range.integral,
            floor: linears_of(range.floor.forms(), &parameter)?, ceiling: linears_of(range.ceiling.forms(), &parameter)? })
    }

    /// The reach of the access at unit PC `pc` proved by the entry of the loop `ends` names, where that loop
    /// is the access's own (its header is `ends.header` in the function) and writes none of the forms' registers,
    /// and the access's array is hoisted at or outside it. `None` where any of that fails.
    pub(crate) fn affine_resolve(&self, nest: Nest<'_>, pc: usize, ends: &Ends) -> Option<Resolved> {
        let (header, back) = nest.around(pc).into_iter().find(|&(header, _)| self.caller_pc(header) == Some(ends.header))?;
        let (array, _) = access_registers(nest.instructions[pc])?;
        let array = self.loop_array(pc, Some(array))?;
        if array.header > header { return None; }
        let registers: Vec<Register> = ends.floor.iter().chain(ends.ceiling.iter()).flatten()
            .flat_map(|form| form.terms.iter().flatten().map(|&(register, _)| register)).collect();
        if !nest.unwritten((header, back), &registers) { return None; }
        let reach = Reach::Affine { floor: ends.floor, ceiling: ends.ceiling };
        Some(Resolved { version: Version { pc, header, reach }, add: None, induction: None })
    }

    /// Continues where every floor form is at least 0 and every ceiling form is below `length` (the array's
    /// extent on the entering edge), else at `exit`. A form is `i64` arithmetic over the entering edge's int32
    /// registers, exact by `COEFFICIENT_LIMIT` and `OFFSET_LIMIT`.
    pub(super) fn affine_within(&mut self, state: &mut State, length: ir::Value, (floor, ceiling): (Linears, Linears),
        exit: ir::Block) -> ir::Value {
        let zero = self.raw_const(Lane::I64, 0);
        let mut within: Option<ir::Value> = None;
        let ends = floor.into_iter().flatten().map(|form| (form, true)).chain(ceiling.into_iter().flatten().map(|form| (form, false)));
        for (form, is_floor) in ends {
            let word = self.linear_word(state, form, exit);
            let check = if is_floor { self.raw_compare(Relation::SignedLessEqual, zero, word) } else { self.raw_compare(Relation::UnsignedLess, word, length) };
            within = Some(match within {
                Some(held) => self.raw_binary(Binary::And, held, check),
                None => check,
            });
        }
        within.expect("an index range's ends each hold a form")
    }

    /// `form`'s value on the entering edge as a signed 64-bit word.
    fn linear_word(&mut self, state: &mut State, form: Linear, exit: ir::Block) -> ir::Value {
        let mut word = self.raw_const(Lane::I64, form.offset);
        for (register, coefficient) in form.terms.into_iter().flatten() {
            let value = self.entry_register(state, register, exit);
            let scaled = self.raw_binary_imm(Binary::Mul, value, coefficient);
            word = self.raw_binary(Binary::Add, word, scaled);
        }
        word
    }

    /// The function PC of the caller's own instruction at unit PC `unit_pc`; None for an inlined callee's.
    fn caller_pc(&self, unit_pc: usize) -> Option<u32> {
        match self.unit.origins.get(unit_pc)? {
            crate::compiled::lane_unit::Origin::Caller(pc) => Some(*pc),
            crate::compiled::lane_unit::Origin::Inlined { .. } => None,
        }
    }
}
