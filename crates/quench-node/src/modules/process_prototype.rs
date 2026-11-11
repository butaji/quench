//! `process`'s prototype chain (lib/internal/bootstrap/node.js): `process` is an object of a
//! function named `process`, and that function's prototype inherits `EventEmitter.prototype`.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

/// Gives `process` its own prototype, which holds only `constructor` and inherits
/// `emitter_prototype`, as Node sets `Object.getPrototypeOf(process)`'s prototype.
pub(crate) fn inherit_emitter(c: &mut NativeContext<'_, NodeHost>, process: RootId, emitter_prototype: RootId) -> Result<(), RootedError> {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation!("processConstructor"), "process")?;
    let prototype = c.get_named_property_rooted(constructor, "prototype")?;
    c.set_prototype_rooted(prototype, emitter_prototype)?;
    c.set_prototype_rooted(process, prototype).map(drop)
}
