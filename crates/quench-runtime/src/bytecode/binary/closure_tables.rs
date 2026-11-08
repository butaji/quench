//! The tables of what a program's closures close over: each function's closure reads and
//! each nested first-call stub's scopes, which an encoding carries with the records.
use super::{BinaryReader, BinaryWriter};

/// Writes `program`'s closure tables after its records (`read_closure_tables`).
pub(super) fn write_closure_tables(out: &mut BinaryWriter, program: &crate::bytecode::ResidualProgram) {
    write_closure_reads(out, program.closure_reads.as_deref());
    write_first_call_scopes(out, &program.first_call_scopes);
}

/// The closure tables `write_closure_tables` wrote, for a program of `functions` functions.
pub(super) fn read_closure_tables(input: &mut BinaryReader<'_>, functions: usize)
    -> Result<(Option<std::rc::Rc<crate::bytecode::ClosureReads>>, crate::compile::first_call::FirstCallStubs), String> {
    Ok((read_closure_reads(input)?, read_first_call_scopes(input, functions)?))
}

/// A program's closure reads (`ClosureReads`), when it has them: per function its parent,
/// whether it is an arrow, its reads (none for its whole chain) and its by-name slots.
fn write_closure_reads(out: &mut BinaryWriter, reads: Option<&crate::bytecode::ClosureReads>) {
    out.u8(u8::from(reads.is_some()));
    let Some(reads) = reads else {
        return;
    };
    out.u32(reads.functions.len() as u32);
    for function in reads.functions.iter() {
        out.option_u32(function.parent);
        out.u8(u8::from(function.is_arrow));
        out.u8(u8::from(function.reads.is_some()));
        if let Some(pairs) = &function.reads {
            out.u32(pairs.len() as u32);
            for &(distance, slot) in pairs.iter() {
                out.u32(distance);
                out.u16(slot);
            }
        }
        out.u16s(&function.named_slots);
    }
}

fn read_closure_reads(input: &mut BinaryReader<'_>) -> Result<Option<std::rc::Rc<crate::bytecode::ClosureReads>>, String> {
    if input.u8()? == 0 {
        return Ok(None);
    }
    let functions = input.list(|input| {
        let parent = input.option_u32()?;
        let is_arrow = input.u8()? != 0;
        let reads = if input.u8()? != 0 {
            Some(input.list(|input| Ok((input.u32()?, input.u16()?)))?.into_boxed_slice())
        } else {
            None
        };
        let named_slots = input.u16s()?.into_boxed_slice();
        Ok(crate::bytecode::closure_reads::FunctionReads { parent, is_arrow, reads, named_slots })
    })?;
    if functions
        .iter()
        .any(|function| function.parent.is_some_and(|parent| parent as usize >= functions.len()))
    {
        return Err("invalid residual closure reads parent".into());
    }
    Ok(Some(std::rc::Rc::new(crate::bytecode::ClosureReads { functions: functions.into_boxed_slice() })))
}

/// The scopes of a program's nested first-call stubs (`ResidualProgram::first_call_scopes`):
/// per stub its id, inherited strictness and levels, innermost first, of names and slots.
fn write_first_call_scopes(out: &mut BinaryWriter, stubs: &crate::compile::first_call::FirstCallStubs) {
    out.u32(stubs.len() as u32);
    for (stub, scopes) in stubs.iter() {
        out.u32(*stub);
        out.bool(scopes.strict());
        out.u32(scopes.levels().len() as u32);
        for level in scopes.levels() {
            out.u32(level.len() as u32);
            for (name, slot) in level.iter() {
                out.string(name);
                out.u16(*slot);
            }
        }
    }
}

fn read_first_call_scopes(input: &mut BinaryReader<'_>, functions: usize) -> Result<crate::compile::first_call::FirstCallStubs, String> {
    let stubs = input.list(|input| {
        let stub = input.u32()?;
        let strict = input.bool()?;
        let levels = input.list(|input| {
            let level = input.list(|input| Ok((Box::<str>::from(input.string()?), input.u16()?)))?;
            Ok(std::rc::Rc::from(level))
        })?;
        Ok((stub, std::rc::Rc::new(crate::compile::first_call::FirstCallScopes::new(levels.into_boxed_slice(), strict))))
    })?;
    if stubs.iter().any(|(stub, _)| *stub as usize >= functions) || !stubs.is_sorted_by_key(|(stub, _)| *stub) {
        return Err("invalid residual first-call stub".into());
    }
    Ok(stubs.into_boxed_slice())
}
