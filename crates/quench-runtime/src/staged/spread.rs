//! The calls whose arguments the VM gathers, as call-out rows: a call with a spread argument
//! (`CallSpread`, `CallArray`) and `super(...)`. Their arguments are an iterable to spread (guest
//! code), an array to apply, or registers a construction passes; callee, receiver and argument
//! registers are operands (`CallOut::operands`), and the VM's call or construction (`Vm::call_out`)
//! is the one definition every tier runs.

use super::callout::calling;
use super::{NumericFrame, NumericStage, Operation, Step};
#[cfg(not(stencil_build))]
use super::Arguments;
#[cfg(not(stencil_build))]
use crate::bytecode::{ConstructArguments, Op, WideInstruction};

/// What the argument registers of a call-out call hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Passing {
    /// The values to pass, in the registers of the window.
    Values,
    /// One register holding an array whose elements are the arguments.
    Array,
    /// One register holding an iterable, whose values are spread into the arguments.
    Iterable,
    /// One register holding a rest parameter kept as a view (`CallRest`): its array, spread as
    /// `Iterable` spreads one, or `undefined`, for the values the activation kept past its fixed
    /// parameters.
    Rest,
}

#[cfg(not(stencil_build))]
/// The call-out row of a spread call or a super construction; `None` for any other instruction,
/// and for a direct eval, which the host decides.
#[cfg(not(stencil_build))]
#[inline(always)]
pub(super) fn decode<W, P>(instruction: WideInstruction) -> Option<Operation<W, P>> {
    let destination = || instruction.result_register();
    Some(match instruction.op() {
        Op::CallSpread | Op::CallArray | Op::CallRest if !instruction.direct_eval() && !instruction.parameter_eval() => {
            let window = instruction.call_window();
            let passing = match instruction.op() {
                Op::CallSpread => Passing::Iterable,
                Op::CallRest => Passing::Rest,
                _ => Passing::Array,
            };
            Operation::SpreadCall { destination: destination(), callee: instruction.register_b(), receiver: instruction.register_c(),
                arguments: Arguments::Window { first: window.base, count: window.count }, passing }
        }
        Op::Construct if instruction.is_super_construct() => {
            let (arguments, passing) = match instruction.construct_arguments() {
                ConstructArguments::Registers(window) => (Arguments::Window { first: window.base, count: window.count }, Passing::Values),
                ConstructArguments::Array(register) => (Arguments::Window { first: register, count: 1 }, Passing::Array),
            };
            Operation::SuperCall { destination: destination(), callee: instruction.register_b(), arguments, passing }
        }
        _ => return None,
    })
}

/// Run a spread call or a super construction: the VM's own, defining `destination`.
#[inline(always)]
pub(super) fn step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation.call_out() {
        Some((call, destination)) => calling(stage, frame, call, destination),
        None => Step::Unresolved,
    }
}

/// `step_of` for any operation.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::ImmediateLayout;
    use crate::staged::{CallOut, DecodeContext};

    type Row = Operation<u64, ()>;

    fn decoded(op: Op, imm: u32) -> Option<Row> {
        Row::decode(WideInstruction::new(op, 4, 5, 6, imm), DecodeContext::EMPTY)
    }

    #[test]
    fn a_spread_call_names_its_callee_receiver_and_one_argument_register() {
        let imm = ImmediateLayout::call_immediate(7, 1, false, false);
        for (op, passing) in [(Op::CallSpread, Passing::Iterable), (Op::CallArray, Passing::Array), (Op::CallRest, Passing::Rest)] {
            let (call, result) = decoded(op, imm).and_then(|row| row.call_out()).expect("a spread call is a call-out row");
            let arguments = Arguments::Window { first: 7, count: 1 };
            assert_eq!(call, CallOut::Spread { callee: 5, receiver: 6, arguments, passing }, "{op:?}");
            assert_eq!(result, Some(4));
            let operands = call.operands();
            assert_eq!((operands.fixed, operands.list), ([Some(crate::bytecode::Operand::register(5)), Some(crate::bytecode::Operand::register(6))], Some(arguments)));
            assert_eq!(operands.registers(&[]).collect::<Vec<_>>(), [5, 6, 7]);
        }
        let eval = ImmediateLayout::call_immediate(7, 1, true, false);
        assert!(decoded(Op::CallSpread, eval).is_none(), "an eval is the host's");
    }

    #[test]
    fn a_super_construction_passes_a_window_of_values_or_an_array() {
        let values = decoded(Op::Construct, ImmediateLayout::construct_immediate(2, true, false)).and_then(|row| row.call_out());
        let (call, _) = values.expect("a super construction is a call-out row");
        assert_eq!(call, CallOut::SuperCall { callee: 5, arguments: Arguments::Window { first: 6, count: 2 }, passing: Passing::Values });
        assert_eq!(call.operands().registers(&[]).collect::<Vec<_>>(), [5, 6, 7]);
        let array = decoded(Op::Construct, ImmediateLayout::construct_immediate(0, true, true)).and_then(|row| row.call_out());
        let (call, _) = array.expect("an applied super construction is a call-out row");
        assert_eq!(call, CallOut::SuperCall { callee: 5, arguments: Arguments::Window { first: 6, count: 1 }, passing: Passing::Array });
        assert!(decoded(Op::Construct, ImmediateLayout::construct_immediate(2, false, false)).is_some_and(|row| row.call_out().is_none()),
            "an ordinary construction is a call, not a call-out");
    }
}
