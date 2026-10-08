use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const MODULE: &str = r#"({
  startupSnapshot: Object.freeze({
    isBuildingSnapshot() { return false; },
  }),
})"#;

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted(MODULE, "node:v8/shared-vm.js")
}
