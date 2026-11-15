//! `t.assert`: the assertion methods bound to a test's plan, each counting itself in the node.

use super::{function, get, put, with_state, Context, NodeId};
use quench_runtime::{RootId, RootedError};

/// The assertion methods `t.assert` binds to the test's plan.
const PLANNED_ASSERTIONS: [&str; 18] = [
    "deepEqual",
    "deepStrictEqual",
    "doesNotMatch",
    "doesNotReject",
    "doesNotThrow",
    "equal",
    "fail",
    "ifError",
    "match",
    "notDeepEqual",
    "notDeepStrictEqual",
    "notEqual",
    "notStrictEqual",
    "partialDeepStrictEqual",
    "rejects",
    "strictEqual",
    "throws",
    "ok",
];

pub(super) fn assert(c: &mut Context<'_>, id: NodeId) -> Result<RootId, RootedError> {
    if let Some(assert) = with_state(c, |state| state.tree[id].assert) {
        return Ok(assert);
    }
    let module = crate::modules::assert_shared_vm::module(c)?;
    let object = c.object_rooted()?;
    let node = c.number(id as f64);
    for name in PLANNED_ASSERTIONS {
        let method = get(c, module, name)?;
        if !c.is_callable_rooted(method)? {
            continue;
        }
        let data = c.array_rooted(&[method, node])?;
        let wrapper = function(c, "nodeTestAssert", name, data)?;
        put(c, object, name, wrapper)?;
    }
    let object = c.retain(object)?;
    with_state(c, |state| state.tree[id].assert = Some(object));
    Ok(object)
}

pub(crate) fn planned_assert(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let method = get(c, data, "0")?;
    let node = get(c, data, "1")?;
    let id = c.rooted_value(node).and_then(|value| value.as_number()).unwrap_or_default() as NodeId;
    with_state(c, |state| state.tree[id].assertions += 1);
    let undefined = c.undefined();
    c.call_rooted(method, undefined, args)
}
