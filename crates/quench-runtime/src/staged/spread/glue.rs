//! Runtime glue of `spread`: the decode of a spread call or a super construction from a record.
#![allow(unused_imports)]
use super::*;


#[cfg(not(stencil_build))]
/// The call-out row of a spread call or a super construction; `None` for any other instruction,
/// and for a direct eval, which the host decides.
#[cfg(not(stencil_build))]
#[inline(always)]
pub fn decode<W, P>(instruction: WideInstruction) -> Option<Operation<W, P>> {
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

