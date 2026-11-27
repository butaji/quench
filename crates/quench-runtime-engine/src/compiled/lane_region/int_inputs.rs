//! Entry types the AOT image's closed world proves: a local or a parameter its closed world types
//! a Number at every read and write takes that type where the loop is entered, proved once by the
//! solve rather than checked at entry.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// The locals of `function` (`function_id`) its AOT image's closed world proves Numbers at every
/// read and write (`AotFacts::numbers`), added to the annotated locals (`declared::declared_local_types`)
/// as one source of local types: each is conformed where it is written, so a join keeps it unboxed.
/// Parameters enter typed instead (`closed_entry_types`); a register an annotation types keeps it.
pub(crate) fn closed_local_types(vm: VmFacts<'_>, function_id: u32, function: &quench_runtime::bytecode::Function, locals: &mut Vec<(Register, Type)>) {
    let Some(numbers) = vm.facts.and_then(|closed| closed.function_facts(function_id).numbers()) else { return };
    let parameters: Vec<usize> = (0..usize::from(function.params)).filter_map(|index| super::scalar::parameter_register(function, index).map(usize::from)).collect();
    for (register, number) in numbers.iter().enumerate() {
        let ty = match number {
            Some(Representation::Int32) => Type::INT32,
            Some(Representation::Float64) => Type::NUMBER,
            _ => continue,
        };
        let Ok(register) = Register::try_from(register) else { continue };
        if parameters.contains(&usize::from(register)) || locals.iter().any(|(local, _)| *local == register) { continue; }
        locals.push((register, ty));
    }
}

/// The entry types of `function`'s (`function_id`'s) parameters its AOT image's closed world proves
/// Numbers at every read and write (`AotFacts::numbers`): an int32 or a Number, as a declared
/// parameter's type enters (`scalar::declared_entry_types`), so the entry proves each once and the
/// body holds it unboxed. Nothing without a closed world (`VmFacts::closed`).
pub(crate) fn closed_entry_types(vm: VmFacts<'_>, function_id: u32, function: &quench_runtime::bytecode::Function, initial: &mut [Type]) {
    let Some(numbers) = vm.facts.and_then(|closed| closed.function_facts(function_id).numbers()) else { return };
    for index in 0..usize::from(function.params) {
        let Some(register) = super::scalar::parameter_register(function, index).map(usize::from) else { continue };
        let ty = match numbers.get(register) {
            Some(Some(Representation::Int32)) => Type::INT32,
            Some(Some(Representation::Float64)) => Type::NUMBER,
            _ => continue,
        };
        if let Some(entry) = initial.get_mut(register).filter(|entry| **entry == Type::ANY) {
            *entry = ty;
        }
    }
}
