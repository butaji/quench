//! The arguments a call passes as Numbers, planned into its direct row (`CallRecord::proven_args`): the facts analysis
//! (`facts::analyze_text`) run over the enclosing function's bytecode from pc 0, every register starting unknown except the
//! declared Number parameters (`declared::entry_numbers`) and the pinned webs the body's entry guards at its header
//! (`entry_pins`: int32s and doubles), which every entry checks before pc 0 runs. A call's argument is proven where its register holds a Number at the call on every path. The analysis is an
//! invariant of the bytecode, as `exact.rs` relies on: a region or resume stub enters only in states it covers.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::facts::{self, Seeds, Text};
use super::super::lane_region::writes;
use super::call_plan::{CallForm, CallShape};
use super::link::INT32_BIT;
use super::plan::Homes;
use super::span::Span;
use super::webs::Webs;
use quench_runtime::bytecode::{Constant, Function, Register, WideInstruction};
use quench_runtime::typed_ir::Type;

/// The frame registers of the pinned int32 and double webs the body's entry guards at its header (`entry_rows`'s pin
/// rows, which every entry runs): `(int32s, doubles)`.
pub(crate) fn entry_pins(homes: &Homes, webs: &Webs, span: &Span) -> (Vec<Register>, Vec<Register>) {
    let entered = webs.entry_live(span);
    let held = |web: &Register| entered.contains(&usize::from(*web));
    let ints = homes.pins.iter().filter(|(_, web)| held(web)).map(|(_, web)| homes.orig[usize::from(*web)]).collect();
    let doubles = homes.floats.iter().filter(|(_, web)| held(web)).map(|(_, web)| homes.orig[usize::from(*web)]).collect();
    (ints, doubles)
}

/// Per PC of `function`, the mask of its call's arguments proven Numbers (bit `k`: argument `k`) and int32s (bit
/// `INT32_BIT + k`, which implies bit `k`), zero where no call is
/// or no argument is proven; `None` where the analysis models less than the function does (`exact::exact_pcs`'s conditions).
pub(crate) fn proven_arguments(function: &Function, constants: &[Constant], numbers: &[Register], ints: &[Register]) -> Option<Box<[u64]>> {
    if !function.handlers.is_empty() || function.is_generator || function.is_async || function.has_dynamic_scope_access() || function.arguments_slot.is_some() {
        return None;
    }
    let instructions: Vec<WideInstruction> = function.instructions().collect();
    let blocks = quench_runtime::bytecode::instruction_leaders(&instructions);
    let mut initial = vec![Type::ANY; usize::from(function.registers)];
    for &register in numbers {
        initial[usize::from(register)] = Type::NUMBER;
    }
    for &register in ints {
        initial[usize::from(register)] = Type::INT32;
    }
    let text = Text { entry: 0, instructions: &instructions, leaders: &blocks, constants, written: &writes,
        own_shape: &|_| None, native_calls: true, raises: &[] };
    let seeds = Seeds { contracts: &[], model: None, signless: &[] };
    let mut masks = vec![0u64; instructions.len()];
    facts::analyze_text(&text, &initial, seeds).replay(&text, seeds, |pc, point| {
        let instruction = instructions[pc];
        if CallForm::of(instruction) != Some(CallForm::Plain) {
            return;
        }
        let shape = CallShape::of(pc as u32, instruction, CallForm::Plain);
        masks[pc] = (0..u32::from(shape.argc).min(INT32_BIT)).fold(0u64, |mask, k| {
            let register = shape.window + k as Register;
            let number = if point.holds_number(register) { 1u64 << k } else { 0 };
            let int32 = if point.holds_int32(register) { 1u64 << (INT32_BIT + k) } else { 0 };
            mask | number | int32
        });
    });
    Some(masks.into_boxed_slice())
}

/// The proven-argument masks of one compile (`proven_arguments`), made on the first direct call that reads them and
/// kept on the function for its seeds (`FunctionCaches::proven_args`): a compile with no direct call runs no analysis,
/// and a recompile from the same seeds reruns none. The analysis reads the bytecode and the seeds alone.
pub(crate) struct ProvenArgs<'a> {
    pub(crate) function: &'a Function,
    pub(crate) constants: &'a [Constant],
    pub(crate) numbers: Vec<Register>,
    pub(crate) ints: Vec<Register>,
    masks: std::cell::OnceCell<std::rc::Rc<[u64]>>,
}

impl<'a> ProvenArgs<'a> {
    pub(crate) fn new(function: &'a Function, constants: &'a [Constant], mut numbers: Vec<Register>, mut ints: Vec<Register>) -> Self {
        (numbers.sort_unstable(), ints.sort_unstable());
        (numbers.dedup(), ints.dedup());
        Self { function, constants, numbers, ints, masks: std::cell::OnceCell::new() }
    }

    /// The mask of the call at `pc`.
    pub(crate) fn at(&self, pc: usize) -> u64 {
        self.masks.get_or_init(|| {
            let cache = &self.function.caches().proven_args;
            let key = (self.numbers.as_slice(), self.ints.as_slice());
            if let Some(hit) = cache.borrow().iter().find(|(n, i, _)| (&n[..], &i[..]) == key) { return hit.2.clone(); }
            let masks: std::rc::Rc<[u64]> = proven_arguments(self.function, self.constants, &self.numbers, &self.ints).unwrap_or_default().into();
            let mut cache = cache.borrow_mut();
            if cache.len() >= quench_runtime::bytecode::PROVEN_ARGS_SEEDS { cache.remove(0); }
            cache.push((self.numbers.clone().into(), self.ints.clone().into(), masks.clone()));
            masks
        }).get(pc).copied().unwrap_or(0)
    }
}
