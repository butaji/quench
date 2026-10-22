//! Shared-VM crypto surface used by CommonJS packages.

use crate::host::NodeHost;
use md5::Md5;
use rqj::{NativeContext, RootId, RootedError};
use sha2::digest::Digest;

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(md5) => ({
  createHash(algorithm) {
    if (String(algorithm).toLowerCase() !== "md5") {
      throw new Error(`Digest method not supported: ${algorithm}`);
    }
    let input = "";
    return {
      update(value, encoding) {
        if (typeof value !== "string") {
          if (value && typeof value.toString === "function") {
            value = value.toString(encoding || "utf8");
          } else {
            value = String(value);
          }
        }
        input += value;
        return this;
      },
      digest(encoding) {
        if (encoding !== undefined && String(encoding).toLowerCase() !== "hex") {
          throw new Error(`Unknown encoding: ${encoding}`);
        }
        return md5(input);
      },
    };
  },
})"#
);

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(FACTORY, "node:crypto/shared.js")?;
    let md5 = context.host_function(crate::host::shared_vm::operation("cryptoMd5"))?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[md5])
}

pub(crate) fn md5(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = match args.first().copied() {
        Some(value) => context.to_string(value)?,
        None => String::new(),
    };
    let digest = Md5::digest(input.as_bytes());
    Ok(context.string_rooted(&format!("{digest:x}")))
}
