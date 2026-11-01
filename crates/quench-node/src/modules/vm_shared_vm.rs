//! Compatibility surface for Node's `vm` module.
//!
//! Each context is a runtime realm with its own ECMAScript intrinsics. Scripts
//! run as global Scripts of their realm, so top-level declarations persist
//! across evaluations as in Node. Node intercepts every global access and
//! forwards it to the sandbox; here the sandbox's own properties are copied
//! onto the realm global before each evaluation and global changes copied
//! back after it, while the global's prototype forwards other names to the
//! sandbox live.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};


pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = crate::host::removed_javascript("node:vm/shared.js")?;
    let create_realm = context.host_function(crate::host::shared_vm::operation("vmCreateRealm"))?;
    let evaluate = context.host_function(crate::host::shared_vm::operation("vmEvaluateScript"))?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[create_realm, evaluate])
}

pub(crate) fn create_realm(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.create_realm_rooted()
}

/// `(global, source, filename)`: evaluate `source` as a global Script of `global`'s realm.
pub(crate) fn evaluate_script(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let [global, source, filename] = args else {
        return Err(RootedError::host(
            "vm script evaluation expects three arguments",
        ));
    };
    let source = context.to_string(*source)?;
    let filename = context.to_string(*filename)?;
    context.evaluate_script_in_realm_rooted(*global, &source, &filename)
}
