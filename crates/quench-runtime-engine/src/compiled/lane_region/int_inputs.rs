//! Entry types the AOT image's closed world proves: a local or a parameter its closed world types
//! a Number at every read and write takes that type where the loop is entered, proved once by the
//! solve rather than checked at entry.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// The locals of `function` (`function_id`) its AOT image's closed world proves Numbers at every
/// read and write (`AotFacts::numbers`), added to the annotated locals (`declared::declared_local_types`)
/// as one source of local types: each is conformed where it is written, so a join keeps it unboxed.
/// Parameters enter typed instead (`solved_entry_types`); a register an annotation types keeps it.
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

/// The entry types the solve proves of `function`'s (`function_id`'s) parameters the unit never writes: the cell the
/// settled solve gives each parameter at the entry pc `entry` (`FunctionFacts::cell_at`), where that cell is an int32 or
/// a Number. Nothing is checked at entry: a parameter the solve proves nothing of keeps its entry type.
pub(crate) fn solved_entry_types(facts: Option<&quench_runtime::compile::aot_facts::AotFacts>, function_id: u32,
    function: &quench_runtime::bytecode::Function, entry: u32, written: &quench_runtime::dataflow::BitSet, initial: &mut [Type]) {
    let Some(facts) = facts else { return };
    let solved = facts.function_facts(function_id);
    for index in 0..usize::from(function.params) {
        let Some(register) = super::scalar::parameter_register(function, index) else { continue };
        let register = usize::from(register);
        if written.contains(register) { continue; }
        let Some(cell) = solved.cell_at(entry, register as u16) else { continue };
        let numeric = cell.ty.is_subtype(Type::INT32) || cell.ty.is_subtype(Type::NUMBER);
        if let Some(slot) = initial.get_mut(register).filter(|slot| **slot == Type::ANY && numeric) {
            *slot = cell.ty;
        }
    }
}
