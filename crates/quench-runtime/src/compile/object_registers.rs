//! Whole-function scalar replacement of unobservable own data properties.
//!
//! The residual program itself owns the promoted fields, so interpreter recovery
//! and native execution see the same registers and need no virtual-object cache.
use super::CompileScratch;
use super::liveness::{self, MethodArguments, RegisterSet, register_bit};
use crate::bytecode::{FieldLookup, FieldSite, Function, Instr, ObjectSite, REGISTER_MOVE, Superinstruction, Traits};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Aliases {
    may: RegisterSet,
    must: RegisterSet,
}

pub(super) fn promote(
    function: &mut Function,
    methods: &dyn MethodArguments,
    fields: &[FieldSite],
    supers: &[Superinstruction],
    objects: &[ObjectSite],
    scratch: &mut CompileScratch,
) {
    // Exceptional/suspended entries and dynamic binding observers need their own
    // object materialization proof. Keep their original residual program.
    if !function.wide.is_empty() || !function.handlers.is_empty()
        || function.is_async || function.is_generator || function.has_dynamic_scope_access()
    { return; }
    let mut pc = 0;
    while pc < function.code.len() {
        if let Some(width) = literal_values(function.code[pc]).map(|values| values.len() as u16)
            && let Some(replacements) = replacement(function, pc, methods, fields, supers, objects, scratch)
        {
            let old = std::mem::take(&mut function.code);
            let mut code = scratch.code_buffer(old.len() + 1);
            let map = scratch.remap(old.len());
            for (index, (instruction, replacement)) in old.iter().zip(replacements).enumerate() {
                map[index] = code.len();
                match replacement {
                    Some(instructions) => code.extend(instructions),
                    None => code.push(*instruction),
                }
            }
            map[old.len()] = code.len();
            super::rewrite::relocate(&mut code, &mut function.wide, &*map, &mut function.handlers,
                &mut function.parameter_end_pc, &mut function.binding_sites,
                &mut function.source_positions, &old);
            let next = map[pc] + usize::from(width);
            function.code = code;
            scratch.recycle_code(old);
            function.registers += width;
            pc = next;
        } else { pc += 1; }
    }
}

/// The registers a plain object literal's allocation takes its values from, in its object
/// site's atom order: a pair's two operands, or a literal's register window. `None` for any
/// other instruction.
fn literal_values(make: Instr) -> Option<smallvec::SmallVec<[u16; 4]>> {
    let op = make.op();
    if op.traits().has(Traits::PAIR_LITERAL_MAKE) {
        Some(smallvec::smallvec![make.b(), make.c()])
    } else if op.traits().has(Traits::WINDOW_LITERAL_MAKE) {
        let window = make.register_window();
        Some((window.base..window.base + window.count).collect())
    } else {
        None
    }
}

fn replacement(
    function: &Function, allocation: usize, methods: &dyn MethodArguments,
    fields: &[FieldSite], supers: &[Superinstruction], objects: &[ObjectSite],
    scratch: &mut CompileScratch,
) -> Option<Vec<Option<smallvec::SmallVec<[Instr; 2]>>>> {
    // Reuse the authoritative register-transfer domain. Exhausting its mask
    // representation leaves the original allocation intact.
    let make = function.code[allocation];
    let values = literal_values(make)?;
    if function.registers.checked_add(values.len() as u16)? > liveness::ROOT_MAP_REGISTERS { return None; }
    if make.result_flags() != 0 { return None; }
    let atoms = &objects.get(make.object_site_index())?.atoms;
    let distinct = atoms.iter().enumerate().all(|(index, atom)| !atoms[..index].contains(atom));
    if atoms.is_empty() || atoms.len() != values.len() || !distinct { return None; }
    // The tables are the scratch's own buffers, filled in place for this function.
    if !liveness::buffers::transfers_into(function, methods, fields, supers, &mut scratch.transfers) { return None; }
    scratch.successors.refill(function);
    scratch.incoming.clear();
    scratch.incoming.resize(function.code.len(), None);
    scratch.incoming[0] = Some(Aliases { may: RegisterSet::default(), must: RegisterSet::default() });
    loop {
        let mut changed = false;
        for (pc, instruction) in function.instructions().enumerate() {
            let Some(before) = scratch.incoming[pc] else { continue };
            let definitions = scratch.transfers[pc].1;
            let mut after = Aliases { may: before.may & !definitions, must: before.must & !definitions };
            if pc == allocation {
                after.may |= register_bit(make.result_register());
                after.must |= register_bit(make.result_register());
            } else if instruction.op() == REGISTER_MOVE {
                let source = register_bit(instruction.b());
                let destination = register_bit(instruction.result_register());
                if before.may & source != 0 { after.may |= destination; }
                if before.must & source != 0 { after.must |= destination; }
            }
            for (target, _) in scratch.successors.at(pc) {
                let Some(entry) = scratch.incoming.get_mut(target) else { continue };
                let next = entry.map_or(after, |old| Aliases {
                    may: old.may | after.may, must: old.must & after.must,
                });
                changed |= *entry != Some(next);
                *entry = Some(next);
            }
        }
        if !changed { break; }
    }
    // Legacy arguments reflection can observe promoted parameter registers even
    // without an explicit bytecode use of the assigned object.
    let observable = if function.has_restricted_legacy_caller_access() { RegisterSet::default() } else {
        function.local_registers.iter().filter(|local| local.local < function.params)
            .fold(RegisterSet::default(), |mask, local| mask | register_bit(local.register))
    };
    // An allocation a loop repeats makes a fresh object each time; the promoted registers
    // stand for the latest one. That is exact while no other register still names an
    // earlier one where the allocation runs again: the only live alias it may find is the
    // result register, which the allocation redefines (dead copies are redefined before use).
    // The solve is needed only when an earlier alias may reach the allocation at all: with none,
    // the test below holds whatever the live set is, and `transfers` already proved it solvable.
    let reached = scratch.incoming[allocation]?;
    let escaping = reached.may & !register_bit(make.result_register());
    if escaping != 0 {
        // The transfer and successor tables above are this function's, so the solve reuses them.
        let transfers = std::mem::take(&mut scratch.transfers);
        let successors = std::mem::take(&mut scratch.successors);
        let live = liveness::analyze_wide_with(function, &transfers, &successors, scratch);
        scratch.transfers = transfers;
        scratch.successors = successors;
        if escaping & live[allocation] != 0 { return None; }
    }
    let mut replacements = vec![None; function.code.len()];
    let scalar = |atom| atoms.iter().position(|candidate| *candidate == atom)
        .map(|index| function.registers + index as u16);
    for (pc, instruction) in function.instructions().enumerate() {
        let Some(aliases) = scratch.incoming[pc] else { continue };
        if aliases.may & observable != 0 { return None; }
        let uses = scratch.transfers[pc].0 & aliases.may;
        if pc == allocation {
            if uses != 0 || register_bit(make.result_register()) & observable != 0 { return None; }
            replacements[pc] = Some(values.iter().enumerate()
                .map(|(index, value)| Instr::new(REGISTER_MOVE, function.registers + index as u16, *value, 0, 0)).collect());
        } else if uses != 0 {
            if uses & !aliases.must != 0 { return None; }
            let op = instruction.op();
            replacements[pc] = Some(if op == REGISTER_MOVE && instruction.result_flags() == 0
                && register_bit(instruction.result_register()) & observable == 0 {
                smallvec::SmallVec::new()
            } else if op.traits().has(Traits::FIELD_LEAF) && instruction.result_flags() == 0 {
                let FieldLookup::Atom { atom, base, .. } = instruction.field_lookup()? else { return None };
                if uses != register_bit(base.register_index()?) { return None; }
                smallvec::smallvec![Instr::new(REGISTER_MOVE, instruction.result_register(), scalar(atom)?, 0, 0)]
            } else if op.traits().has(Traits::NAMED_FIELD_WRITE) {
                if uses != register_bit(instruction.b()) || aliases.may & register_bit(instruction.a()) != 0 { return None; }
                smallvec::smallvec![Instr::new(REGISTER_MOVE, scalar(instruction.imm())?, instruction.a(), 0, 0)]
            } else {
                return None;
            });
        }
    }
    Some(replacements)
}

