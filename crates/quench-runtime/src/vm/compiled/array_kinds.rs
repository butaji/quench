//! The element kind of the typed arrays a loop indexes, where the program itself names one: a
//! register a `new Int32Array(…)` (or any typed-array constructor) defines, moved or held in a
//! local slot, or passed to this function as the argument at a call site that knows it. The kind
//! only selects which element rows the loop lowers with: the region proves it at its entering
//! edge (`prove_typed_view` compares the record's kind and leaves where it differs), so a kind
//! this analysis misses or gets wrong costs an entry exit, never a wrong element.

use super::lane_region::writes;
use crate::bytecode::{Class, Op, Register, ResidualProgram, WideInstruction};
use crate::heap::{TYPED_ARRAYS, TypedArrayKind};

/// The one element kind every array `function`'s loop `header..=backedge` indexes has, where the
/// program names one for all of them; `None` where it names none or several.
pub(in crate::vm::compiled) fn loop_element_kind(program: &ResidualProgram, function: u32, (header, backedge): (u32, u32)) -> Option<TypedArrayKind> {
    let decoded: Vec<WideInstruction> = program.function_get(function as usize)?.instructions().collect();
    let mut found: Option<TypedArrayKind> = None;
    for pc in header as usize..=backedge as usize {
        let instruction = *decoded.get(pc)?;
        if !matches!(instruction.op().class(), Class::GetIndex | Class::SetIndex) { continue; }
        let register = instruction.operand_b().register_index()?;
        let kind = register_kind(program, function, register, pc, &mut Vec::new())?;
        if found.is_some_and(|seen| seen != kind) { return None; }
        found = Some(kind);
    }
    found
}

/// The writer of `register` in `function` before `before`: an instruction of the function's
/// decoded records, with the position it ends at.
fn writer(program: &ResidualProgram, function: u32, register: Register, before: usize) -> Option<(usize, WideInstruction)> {
    let decoded: Vec<WideInstruction> = program.function_get(function as usize)?.instructions().collect();
    (0..before.min(decoded.len())).rev()
        .map(|pc| (pc, decoded[pc]))
        .find(|(_, instruction)| writes(*instruction).contains(&register))
}

/// The typed-array kind `register` holds at `before` in `function`, where the program says one.
fn register_kind(program: &ResidualProgram, function: u32, register: Register, before: usize,
    visiting: &mut Vec<(u32, Register)>) -> Option<TypedArrayKind> {
    if visiting.contains(&(function, register)) { return None; }
    visiting.push((function, register));
    let kind = match writer(program, function, register, before) {
        Some((pc, instruction)) if instruction.op() == Op::Move => {
            register_kind(program, function, instruction.register_b(), pc, visiting)
        }
        Some((pc, instruction)) if instruction.op() == Op::LoadEnvLocal => {
            let slot = instruction.local_slot();
            stored_register(program, function, slot, pc).and_then(|(at, source)| register_kind(program, function, source, at, visiting))
        }
        Some((at, instruction)) if instruction.op() == Op::Construct => constructor_kind(program, function, instruction.register_b(), at),
        None => parameter_kind(program, function, register, visiting),
        _ => None,
    };
    visiting.pop();
    kind
}

/// The register a `StoreEnvLocal` before `before` wrote into `slot`, with its position.
fn stored_register(program: &ResidualProgram, function: u32, slot: usize, before: usize) -> Option<(usize, Register)> {
    let decoded: Vec<WideInstruction> = program.function_get(function as usize)?.instructions().collect();
    (0..before.min(decoded.len())).rev()
        .map(|pc| (pc, decoded[pc]))
        .find(|(_, instruction)| instruction.op() == Op::StoreEnvLocal && instruction.local_slot() == slot)
        .map(|(pc, instruction)| (pc, instruction.register_a()))
}

/// The typed-array kind a construction through the constructor `register` names.
fn constructor_kind(program: &ResidualProgram, function: u32, register: Register, before: usize) -> Option<TypedArrayKind> {
    let (_, instruction) = writer(program, function, register, before)?;
    if instruction.op() != Op::LoadName { return None; }
    let name = program.atoms.get(instruction.atom_index() as usize)?.to_string();
    TYPED_ARRAYS.iter().find(|row| row.name == name).map(|row| row.kind)
}

/// The kind `function`'s parameter `register` is passed at its call sites, where they agree.
fn parameter_kind(program: &ResidualProgram, function: u32, register: Register,
    visiting: &mut Vec<(u32, Register)>) -> Option<TypedArrayKind> {
    let body = program.function_get(function as usize)?;
    if register >= body.params { return None; }
    argument_kind(program, function, usize::from(register), visiting)
}

/// The kind every call site passes `callee`'s argument `index`, where they agree on one.
fn argument_kind(program: &ResidualProgram, callee: u32, index: usize,
    visiting: &mut Vec<(u32, Register)>) -> Option<TypedArrayKind> {
    let mut found: Option<TypedArrayKind> = None;
    for caller in 0..program.function_count() as u32 {
        let decoded: Vec<WideInstruction> = match program.function_get(caller as usize) {
            Some(function) => function.instructions().collect(),
            None => continue,
        };
        for (pc, instruction) in decoded.iter().copied().enumerate() {
            if !matches!(instruction.op().class(), Class::Call | Class::CallKnown) { continue; }
            let target = if instruction.op().class() == Class::CallKnown {
                Some(u32::from(instruction.known_function_index()))
            } else {
                callee_of(program, caller, instruction, pc)
            };
            if target != Some(callee) { continue; }
            let window = instruction.call_window();
            if index >= usize::from(window.count) { continue; }
            let Some(kind) = register_kind(program, caller, window.base + index as u16, pc, visiting) else { continue };
            if found.is_some_and(|seen| seen != kind) { return None; }
            found = Some(kind);
        }
    }
    found
}

/// The program's function of `name`, where exactly one has it.
fn named_function(program: &ResidualProgram, name: &str) -> Option<u32> {
    let mut found = None;
    for index in 0..program.function_count() {
        let Some(atom) = program.function(index).name else { continue };
        let named: &str = program.atoms.get(atom as usize).map_or("", |named| named);
        if named != name { continue; }
        if found.is_some() { return None; }
        found = Some(index as u32);
    }
    found
}

/// The function a call through `instruction` names, where its callee register is a closure
/// made in this function at a statically known function.
fn callee_of(program: &ResidualProgram, caller: u32, instruction: WideInstruction, pc: usize) -> Option<u32> {
    let register = instruction.register_b();
    let found = writer(program, caller, register, pc);
    let (at, defined) = found?;
    let name = match defined.op() {
        Op::MakeClosure => Some(defined.closure_function_index()),
        // A call through a name: the program's one function of that name, where it has one (a
        // function declaration). A shadowed or reassigned binding only costs an entry exit.
        Op::LoadNameCall => named_function(program, program.atoms.get(defined.atom_index() as usize)?),
        Op::LoadEnvLocal => {
            let slot = defined.local_slot();
            let (store, source) = stored_register(program, caller, slot, at)?;
            let (_, made) = writer(program, caller, source, store)?;
            (made.op() == Op::MakeClosure).then(|| made.closure_function_index())
        }
        _ => None,
    }?;
    Some(name)
}

#[cfg(all(test, feature = "aot"))]
mod tests {
    use super::*;

    /// The loop of `function` that indexes an array, as its span.
    fn loop_of(program: &ResidualProgram, function: u32) -> (u32, u32) {
        let body = program.function_get(function as usize).expect("a function");
        let decoded: Vec<WideInstruction> = body.instructions().collect();
        (0..decoded.len())
            .filter(|at| decoded[*at].op() == Op::Jump)
            .find_map(|at| {
                let header = decoded[at].jump_target();
                (header <= at as u32).then_some((header, at as u32))
            })
            .expect("a loop")
    }

    /// A typed array the program constructs is named by the loop that indexes it, through the
    /// call that passes it.
    #[test]
    fn a_constructed_typed_array_names_its_element_kind() {
        let program = crate::Engine::specialize(
            "function sum(a,n){ let s=0; for (let i=0;i<n;i++) s=s+a[i]; return s; }\n\
             const a=new Float64Array(4); print(sum(a,4));",
            "kind.js",
        )
        .expect("a program");
        let span = loop_of(&program, 1);
        assert_eq!(loop_element_kind(&program, 1, span), Some(TypedArrayKind::Float64));
    }

    /// A dense array names no kind: the loop keeps the dynamic view.
    #[test]
    fn a_dense_array_names_no_element_kind() {
        let program = crate::Engine::specialize(
            "function sum(a,n){ let s=0; for (let i=0;i<n;i++) s=s+a[i]; return s; }\n\
             const a=[1,2,3]; print(sum(a,3));",
            "kind-dense.js",
        )
        .expect("a program");
        let span = loop_of(&program, 1);
        assert_eq!(loop_element_kind(&program, 1, span), None);
    }
}

/// The one int32 range every call site passes `callee`'s argument `index`, where each passes a
/// constant the caller's code fixes (a `const n = 128`): a region lowered ahead guards the
/// range once at its entry, so a loop bound the program actually uses is a fact.
pub(in crate::vm::compiled) fn parameter_int_range(program: &ResidualProgram, callee: u32, index: usize) -> Option<crate::typed_ir::IntRange> {
    let mut found: Option<crate::typed_ir::IntRange> = None;
    for caller in 0..program.function_count() as u32 {
        let Some(body) = program.function_get(caller as usize) else { continue };
        let decoded: Vec<WideInstruction> = body.instructions().collect();
        for (pc, instruction) in decoded.iter().copied().enumerate() {
            if !matches!(instruction.op().class(), Class::Call | Class::CallKnown) { continue; }
            let target = if instruction.op().class() == Class::CallKnown {
                Some(u32::from(instruction.known_function_index()))
            } else {
                callee_of(program, caller, instruction, pc)
            };
            if target != Some(callee) { continue; }
            let window = instruction.call_window();
            if index >= usize::from(window.count) { continue; }
            let range = argument_range(program, caller, window.base + index as u16, pc, &decoded)?;
            match found {
                None => found = Some(range),
                Some(seen) if seen == range => {}
                Some(_) => return None,
            }
        }
    }
    found
}

/// The int32 range `register` holds before `pc` in `function`, through copies and constant
/// locals; `None` for any other value.
fn argument_range(program: &ResidualProgram, function: u32, register: Register, before: usize,
    decoded: &[WideInstruction]) -> Option<crate::typed_ir::IntRange> {
    let (at, instruction) = writer(program, function, register, before)?;
    match instruction.op() {
        Op::Move => argument_range(program, function, instruction.register_b(), at, decoded),
        Op::LoadConst => {
            let ty = program.constants.get(instruction.constant_index() as usize).map(super::facts::constant_type)?;
            ty.int_range().filter(|range| range.min == range.max)
        }
        Op::LoadEnvLocal => {
            let (store, source) = stored_register(program, function, instruction.local_slot(), at)?;
            argument_range(program, function, source, store, decoded)
        }
        _ => None,
    }
}
