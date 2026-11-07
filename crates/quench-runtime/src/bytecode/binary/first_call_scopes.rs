//! The encoding of a program's nested first-call stubs' scopes
//! (`ResidualProgram::first_call_scopes`): a stub's chunk compiles against them, so a program
//! decoded from a snapshot or the compile cache needs them as much as a compiled one.
use super::{BinaryReader, BinaryWriter};
use crate::compile::first_call::FirstCallScopes;
use std::rc::Rc;

pub(super) fn write(out: &mut BinaryWriter, table: &[(u32, Rc<FirstCallScopes>)]) {
    out.u32(table.len() as u32);
    for (stub, scopes) in table {
        out.u32(*stub);
        out.u8(u8::from(scopes.strict));
        out.u32(scopes.levels.len() as u32);
        for level in scopes.levels.iter() {
            out.u32(level.len() as u32);
            for (name, slot) in level.iter() {
                out.string(name);
                out.u16(*slot);
            }
        }
    }
}

pub(super) fn read(input: &mut BinaryReader<'_>) -> Result<Box<[(u32, Rc<FirstCallScopes>)]>, String> {
    let table = input.list(|input| {
        let stub = input.u32()?;
        let strict = input.u8()? != 0;
        let levels = input.list(|input| Ok(input.list(|input| Ok((input.string()?.into_boxed_str(), input.u16()?)))?.into()))?;
        Ok((stub, Rc::new(FirstCallScopes { levels: levels.into_boxed_slice(), strict })))
    })?;
    Ok(table.into_boxed_slice())
}
