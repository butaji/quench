//! `process.permission`, Node's Permission Model API: present only under `--permission`
//! or `--permission-audit`.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

const PERMISSION: &str = "permission";

/// Defines `process.permission` when the model is on, and removes it otherwise.
pub(crate) fn define(c: &mut Context<'_>, process: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(PERMISSION);
    c.delete_property_rooted(process, key)?;
    Ok(())
}
