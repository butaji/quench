//! A method call whose site has prototype-held entries (`plan::chain_reads`): each entry's read of the method through the Held
//! row, then the direct row of the callee it answers. Split from `call_plan.rs` for its size.

use super::*;

/// One entry of a method site's chain: the read of the method through the Held row (its stencil and holes), the callee
/// value it answers (the direct row that follows compares it by identity), and the holder whose slot it reads.
pub(crate) struct ChainRead {
    pub(crate) callee: u64,
    pub(crate) holder: u32,
    pub(crate) stencil: &'static Stencil,
    pub(crate) holes: [u64; HOLES],
}

/// The chain of a method call whose site has prototype-held entries (`CallPlan::chain`) that pair with a callee the site
/// reached: for each pair, its read, then its direct row. The read's miss exits two parts on, to the next read, and the
/// last read's to the helper row; the direct row's completion is `taken` to the continuation, and its refusal falls to the
/// next read (or the helper), which is sound as the direct row compares the callee's identity. None where no entry pairs.
pub(crate) fn chained_reads(plan: &CallPlan) -> Result<Option<CallChain>, String> {
    let pairs: Vec<(&ChainRead, &DirectFacts)> = plan.chain.iter()
        .filter_map(|read| plan.direct.iter().find(|facts| facts.callee == read.callee).map(|facts| (read, facts)))
        .collect();
    if pairs.is_empty() {
        return Ok(None);
    }
    let arguments = argument_places(plan);
    let mut parts = plan.spills.iter().map(|&(place, register)| spill_part(place, register)).collect::<Result<Vec<_>, _>>()?;
    let past = parts.len() + 2 * pairs.len() + 1;
    let mut records = Vec::new();
    for (read, facts) in pairs {
        let (mut direct, owned) = direct_part(&plan.shape, facts, arguments, plan.proven_args)?;
        parts.push(part(read.stencil, read.holes, vec![Exit::Ahead(2)]));
        direct.taken = Some(past);
        parts.push(direct);
        records.push(owned);
    }
    parts.push(helper_part(plan.shape.call_pc, plan.helper, None)?);
    for &(place, register) in plan.reloads {
        parts.push(reload_part(place, register)?);
    }
    Ok(Some(CallChain { parts, records, choice: Choice::Direct }))
}
