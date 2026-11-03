//! Whole-function scalar replacement of unobservable own data properties.
//!
//! The residual program itself owns the promoted fields, so interpreter recovery
//! and native execution see the same registers and need no virtual-object cache.
use super::liveness::{self, MethodArguments, RegisterSet, register_bit};
use crate::bytecode::{FieldLookup, FieldSite, Function, Instr, ObjectSite, Op, Superinstruction};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Aliases {
    may: RegisterSet,
    must: RegisterSet,
}

pub(super) fn promote(
    function: &mut Function,
    methods: &dyn MethodArguments,
    fields: &[FieldSite],
    supers: &[Superinstruction],
    objects: &[ObjectSite],
) {
    // Exceptional/suspended entries and dynamic binding observers need their own
    // object materialization proof. Keep their original residual program.
    if !function.wide.is_empty() || !function.handlers.is_empty()
        || function.is_async || function.is_generator || function.has_dynamic_scope_access()
    { return; }
    let mut pc = 0;
    while pc < function.code.len() {
        if function.code[pc].op() == Op::MakeObject2
            && let Some(replacements) = replacement(function, pc, methods, fields, supers, objects)
        {
            let old = std::mem::take(&mut function.code);
            let mut code = Vec::with_capacity(old.len() + 1);
            let mut map = Vec::with_capacity(old.len() + 1);
            for (instruction, replacement) in old.iter().zip(replacements) {
                map.push(code.len());
                match replacement {
                    Some(instructions) => code.extend(instructions),
                    None => code.push(*instruction),
                }
            }
            map.push(code.len());
            super::rewrite::relocate(&mut code, &mut function.wide, &map, &mut function.handlers,
                &mut function.parameter_end_pc, &mut function.binding_sites,
                &mut function.source_positions, &old);
            function.code = code;
            function.registers += 2;
            pc = map[pc] + 2;
        } else { pc += 1; }
    }
}

fn replacement(
    function: &Function, allocation: usize, methods: &dyn MethodArguments,
    fields: &[FieldSite], supers: &[Superinstruction], objects: &[ObjectSite],
) -> Option<Vec<Option<smallvec::SmallVec<[Instr; 2]>>>> {
    // Reuse the authoritative register-transfer domain. Exhausting its mask
    // representation leaves the original allocation intact.
    if function.registers.checked_add(2)? > liveness::ROOT_MAP_REGISTERS { return None; }
    let make = function.code[allocation];
    if make.result_flags() != 0 { return None; }
    let atoms = &objects.get(make.object_site_index())?.atoms;
    if atoms.len() != 2 || atoms[0] == atoms[1] { return None; }
    let transfers = liveness::transfers(function, methods, fields, supers)?;
    let instructions: Vec<_> = function.instructions().collect();
    let mut edges = Vec::new();
    let mut starts = Vec::with_capacity(instructions.len() + 1);
    let mut repeated_allocation = false;
    for (pc, instruction) in instructions.iter().copied().enumerate() {
        starts.push(edges.len());
        function.for_each_successor(pc, instruction, |target, _| {
            repeated_allocation |= pc >= allocation && target <= allocation;
            edges.push(target);
        });
    }
    if repeated_allocation { return None; }
    starts.push(edges.len());
    let mut incoming = vec![None; instructions.len()];
    incoming[0] = Some(Aliases { may: 0, must: 0 });
    loop {
        let mut changed = false;
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            let Some(before) = incoming[pc] else { continue };
            let definitions = transfers[pc].1;
            let mut after = Aliases { may: before.may & !definitions, must: before.must & !definitions };
            if pc == allocation {
                after.may |= register_bit(make.result_register());
                after.must |= register_bit(make.result_register());
            } else if instruction.op() == Op::Move {
                let source = register_bit(instruction.b());
                let destination = register_bit(instruction.result_register());
                if before.may & source != 0 { after.may |= destination; }
                if before.must & source != 0 { after.must |= destination; }
            }
            for target in &edges[starts[pc]..starts[pc + 1]] {
                let Some(entry) = incoming.get_mut(*target) else { continue };
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
    let observable = if function.has_restricted_legacy_caller_access() { 0 } else {
        function.local_registers.iter().filter(|local| local.local < function.params)
            .fold(0, |mask, local| mask | register_bit(local.register))
    };
    incoming[allocation]?;
    let mut replacements = vec![None; instructions.len()];
    let scalar = |atom| atoms.iter().position(|candidate| *candidate == atom)
        .map(|index| function.registers + index as u16);
    for (pc, instruction) in instructions.iter().copied().enumerate() {
        let Some(aliases) = incoming[pc] else { continue };
        if aliases.may & observable != 0 { return None; }
        let uses = transfers[pc].0 & aliases.may;
        if pc == allocation {
            if uses != 0 || register_bit(make.result_register()) & observable != 0 { return None; }
            replacements[pc] = Some(smallvec::smallvec![
                Instr::new(Op::Move, function.registers, make.b(), 0, 0),
                Instr::new(Op::Move, function.registers + 1, make.c(), 0, 0),
            ]);
        } else if uses != 0 {
            if uses & !aliases.must != 0 { return None; }
            replacements[pc] = Some(match instruction.op() {
                Op::Move if instruction.result_flags() == 0
                    && register_bit(instruction.result_register()) & observable == 0 => smallvec::SmallVec::new(),
                Op::GetField if instruction.result_flags() == 0 => {
                    let FieldLookup::Atom { atom, base, .. } = instruction.field_lookup()? else { return None };
                    if uses != register_bit(base.register_index()?) { return None; }
                    smallvec::smallvec![Instr::new(Op::Move, instruction.result_register(), scalar(atom)?, 0, 0)]
                }
                Op::SetField | Op::SetFieldStrict => {
                    if uses != register_bit(instruction.b()) || aliases.may & register_bit(instruction.a()) != 0 { return None; }
                    smallvec::smallvec![Instr::new(Op::Move, scalar(instruction.imm())?, instruction.a(), 0, 0)]
                }
                _ => return None,
            });
        }
    }
    Some(replacements)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(source: &str, promoted: bool) {
        for generic in [false, true] {
            let program = if generic {
                crate::Engine::specialize_unspecialized(source, "object-registers.js")
            } else {
                crate::Engine::specialize(source, "object-registers.js")
            }.unwrap();
            let id = program.functions.iter().position(|f| f.header().name
                .is_some_and(|name| &program.atoms[name as usize] == "subject")).unwrap();
            let body = program.function(id);
            if !generic {
                assert_eq!(body.instructions().any(|i| i.op() == Op::MakeObject2), !promoted,
                    "{}", program.disassemble());
            }
            program.validate().unwrap();
            for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
                let mut runtime = crate::Runtime::new(crate::SystemHost);
                runtime.set_execution_mode(mode);
                runtime.execute(&program).unwrap();
            }
            #[cfg(feature = "aot")]
            {
                let dynamic = (0..program.functions.len())
                    .any(|id| program.function(id).has_dynamic_scope_access());
                let native = crate::NativeJsProgram::compile_eager(program).unwrap();
                let mut runtime = crate::Runtime::new(crate::SystemHost);
                runtime.set_execution_mode(if dynamic { crate::ExecutionMode::Compiled }
                    else { crate::ExecutionMode::NativeOnly });
                runtime.execute_native(&native).unwrap();
            }
        }
    }

    #[test]
    fn promotes_own_fields_through_loop_aliases_and_joins() {
        check(r#"
            function subject(n, seed) {
                const s = { count: n | 0, control: seed | 0 };
                while (s.count > 0) {
                    s.count = (s.count - 1) | 0;
                    if ((s.control & 1) === 0) s.control = s.control >> 1;
                    else s.control = (s.control >> 1) ^ 0xD008;
                }
                return s.control | 0;
            }
            function reference(n, x) {
                n |= 0; x |= 0;
                while (n-- > 0) x = (x >> 1) ^ ((x & 1) ? 0xD008 : 0);
                return x;
            }
            for (let n of [-1,0,1,2,17,257]) for (let seed of [0,1,-1,2147483647,-2147483648]) {
                if (subject(n,seed) !== reference(n,seed)) throw Error('field state');
            }
        "#, true);
    }

    #[test]
    fn preserves_initializer_and_field_value_effect_order() {
        check(r#"
            let events = [];
            function subject(value) {
                'use strict';
                const s = { a: (events.push('a'), value), b: (events.push('b'), 2) };
                s.b = s.a + s.b;
                return s.b | 0;
            }
            const value = { valueOf() { events.push('coerce'); return 5; } };
            if (subject(value) !== 7 || events.join(',') !== 'a,b,coerce') throw Error('order');
        "#, true);
    }

    #[test]
    fn retains_escaping_or_observable_objects() {
        for (body, assertion) in [
            ("return s;", "if (subject().a !== 1) throw Error('return');"),
            ("return (() => s)();", "if (subject().a !== 1) throw Error('capture');"),
            ("eval('s.a = 9'); return s.a | 0;", "if (subject() !== 9) throw Error('eval');"),
            ("Object.defineProperty(s, 'a', {get(){return 9}}); return s.a | 0;", "if (subject() !== 9) throw Error('getter');"),
            ("delete s.a; return s.a;", "if (subject() !== undefined) throw Error('delete');"),
            ("s.a = s; return s.a === s;", "if (!subject()) throw Error('identity');"),
        ] {
            check(&format!("function subject() {{ const s = {{a:1,b:2}}; {body} }} {assertion}"), false);
        }
    }

    #[test]
    fn retains_ambiguous_aliases_at_control_flow_joins() {
        check(r#"
            function subject(flag) {
                const s = {a:1,b:2};
                let alias = s;
                if (flag) alias = {a:7,b:8};
                alias.a = 11;
                return (s.a + alias.b) | 0;
            }
            if (subject(false) !== 13 || subject(true) !== 9) throw Error('alias join');
        "#, false);
    }
}
