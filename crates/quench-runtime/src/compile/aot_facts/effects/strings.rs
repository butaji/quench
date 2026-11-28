//! The facts of the String reads and calls a function makes: the length of a string, and the cell a String builtin
//! defines (`Types::strings`).

use super::super::quiet::StringMethod;
use super::super::value::{Key, Origin, Word};
use super::Ctx;
use crate::bytecode::WideInstruction;
use crate::staged::typing;
use crate::typed_ir::{Cell, IntRange, Type};
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

/// The Number a read of `length` off `owner` is, where `owner` is a string: the integer range of its length
/// (`StringFact::length`). `None` for any other read.
pub(super) fn string_length(ctx: &Ctx<'_>, owner: Option<Word>, atom: u32) -> Option<Type> {
    let fact = owner?.cell.ty.string_fact()?;
    if ctx.scope.length != Some(atom) { return None; }
    let length = fact.length();
    Some(Type::int(IntRange { min: i64::from(length.min), max: i64::from(length.max) }))
}

/// The cell a call of a String builtin defines over the facts before it (`Types::strings`): `String(x)` converts `x`, and
/// `charAt`, `charCodeAt`, `substring` and `slice` of a string receiver are typed by their arguments' types
/// (`typing::char_at` and the rest). A call of a name the program rebinds, or of a receiver not known to be a string,
/// defines none.
pub(super) fn string_call(ctx: &Ctx<'_>, instruction: WideInstruction, state: &[Word]) -> Option<Word> {
    let strings = ctx.types?.strings();
    let invocation = crate::staged::Invocation::of(instruction, ctx.decode)?;
    let arguments: Vec<Type> = invocation.arguments.registers(ctx.records).map(|register| state[usize::from(register)].cell.ty).collect();
    let argument = |index: usize| arguments.get(index).copied().unwrap_or(Type::language(crate::ValueType::Undefined));
    let ty = match invocation.callee {
        crate::staged::Callee::Register(register) => {
            let Some(Origin::Binding(Key::Global(atom))) = state[usize::from(register)].origin else { return None };
            if strings.global != Some(atom) { return None; }
            let [value] = arguments.as_slice() else { return None };
            typing::to_string(*value)
        }
        crate::staged::Callee::Method { atom, .. } => {
            let crate::staged::Receiver::Register(receiver) = invocation.receiver else { return None };
            let receiver = state[usize::from(receiver)].cell.ty;
            receiver.string_fact()?;
            let (_, method) = strings.methods.iter().find(|(named, _)| *named == atom)?;
            match method {
                StringMethod::CharAt => typing::char_at(receiver, argument(0)),
                StringMethod::CharCodeAt => typing::char_code_at(receiver, argument(0)),
                StringMethod::Substring => typing::substring_length(receiver, argument(0), argument(1), false),
                StringMethod::Slice => typing::substring_length(receiver, argument(0), argument(1), true),
            }
        }
        crate::staged::Callee::Known(_) => return None,
    };
    Some(Word::of_cell(Cell::of(ty)))
}
