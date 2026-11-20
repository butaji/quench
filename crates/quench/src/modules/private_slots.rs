//! Internal slots of host classes (Node's `#private` fields): own properties keyed by
//! host-private symbols, which script can neither see nor reach. Each slot's symbol is
//! made once per realm and cached under its name.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

fn symbol(c: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    if let Some(symbol) = c.host_mut().shared_state().borrow().builtin_cache.get(name).copied() {
        return Ok(symbol);
    }
    let symbol = c.private_symbol_rooted(Some(name.trim_start_matches('\0')));
    let symbol = c.retain(symbol)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(name.to_owned(), symbol);
    Ok(symbol)
}

/// The slot's value, or `None` when `object` has no such slot (the receiver check).
pub(crate) fn read(c: &mut Context<'_>, object: RootId, name: &str) -> Result<Option<RootId>, RootedError> {
    if !c.is_object_rooted(object)? {
        return Ok(None);
    }
    let symbol = symbol(c, name)?;
    if c.own_property_fields_rooted(object, symbol)?.is_none() {
        return Ok(None);
    }
    c.get_property_rooted(object, symbol).map(Some)
}

pub(crate) fn write(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let symbol = symbol(c, name)?;
    c.define_data_property_rooted(object, symbol, value, true, false, true)?;
    Ok(())
}
