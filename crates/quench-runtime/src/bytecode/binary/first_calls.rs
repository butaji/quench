//! The encoding of a program's nested first-call stubs (`ResidualProgram::first_calls`).
use super::{BinaryReader, BinaryWriter};
use crate::compile::first_call::{FirstCallScopes, FirstCalls, ScopeLevel};

/// A program's nested first-call stubs: the scope levels they close over, each once, then per
/// stub its id, its strictness and its levels' indices, innermost first.
pub(super) fn write_first_calls(out: &mut BinaryWriter, calls: &FirstCalls) {
    let mut levels: Vec<&ScopeLevel> = Vec::new();
    let mut indices = Vec::with_capacity(calls.len());
    for (_, scopes) in calls.iter() {
        let (chain, _) = scopes.parts();
        indices.push(chain.iter().map(|level| match levels.iter().position(|known| std::rc::Rc::ptr_eq(known, level)) {
            Some(at) => at as u32,
            None => { levels.push(level); (levels.len() - 1) as u32 }
        }).collect::<Vec<u32>>());
    }
    out.u32(levels.len() as u32);
    for level in levels {
        out.u32(level.len() as u32);
        for (name, slot) in level.iter() {
            out.string(name);
            out.u16(*slot);
        }
    }
    out.u32(calls.len() as u32);
    for ((id, scopes), chain) in calls.iter().zip(indices) {
        out.u32(*id);
        out.bool(scopes.parts().1);
        out.u32(chain.len() as u32);
        chain.into_iter().for_each(|at| out.u32(at));
    }
}

pub(super) fn read_first_calls(input: &mut BinaryReader<'_>) -> Result<FirstCalls, String> {
    let levels: Vec<ScopeLevel> =
        input.list(|input| Ok(input.list(|input| Ok((Box::from(input.string()?), input.u16()?)))?.into()))?;
    let calls = input.list(|input| {
        let (id, strict) = (input.u32()?, input.bool()?);
        let chain = input.list(|input| levels.get(input.u32()? as usize).cloned().ok_or_else(|| "invalid residual first-call scope".to_string()))?;
        Ok((id, std::rc::Rc::new(FirstCallScopes::new(chain.into_boxed_slice(), strict))))
    })?;
    if !calls.is_sorted_by(|before, after| before.0 < after.0) {
        return Err("invalid residual first-call order".into());
    }
    Ok(calls.into_boxed_slice())
}
