//! `node:buffer` is Node's lib/buffer.js over `internalBinding('buffer')`; this module
//! gives host code its canonical `Buffer`, `FastBuffer` and prototype.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

const FAST_BUFFER_CACHE: &str = "\0builtin:buffer/FastBuffer";

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

/// Load lib/buffer.js and record the `FastBuffer` every host-created Buffer shares.
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = crate::host::shared_vm::commonjs::node_library_module(c, "buffer.js", "buffer.js")?;
    let shared = c.host_mut().shared_state();
    if !shared.borrow().builtin_cache.contains_key(FAST_BUFFER_CACHE) {
        let fast = crate::host::shared_vm::commonjs::node_library_export(c, "buffer.js", "internal/buffer.js", "FastBuffer")?;
        let fast = c.retain(fast)?;
        shared.borrow_mut().builtin_cache.insert(FAST_BUFFER_CACHE.to_owned(), fast);
    }
    Ok(module)
}

/// Installs `globalThis.Buffer` as Node's accessor pair (non-enumerable getter `get` and
/// setter `set` over one binding) and the canonical `NodeBuffer` alias. The `buffer` module is
/// built on the first read of either, not at start-up.
pub(crate) fn install_global(c: &mut Context<'_>) -> Result<(), RootedError> {
    let load = c.host_function(crate::host::shared_vm::operation("bufferConstructor"))?;
    let installer = c.evaluate_script_rooted(
        r#"(load) => {
  let _Buffer;
  let assigned = false;
  Object.defineProperty(globalThis, "Buffer", {
    get() { return assigned ? _Buffer : load(); },
    set(value) { assigned = true; _Buffer = value; },
    enumerable: false,
    configurable: true,
  });
  Object.defineProperty(globalThis, "NodeBuffer", {
    get: load,
    enumerable: false,
    configurable: true,
  });
}"#,
        "node:buffer/install-global.js",
    )?;
    let undefined = c.undefined();
    c.call_rooted(installer, undefined, &[load])?;
    Ok(())
}

/// The `buffer` module's `Buffer`, building the module on first use.
pub(crate) fn constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let module = crate::host::shared_vm::commonjs::buffer_module(c)?;
    get(c, module, "Buffer")
}

/// The canonical FastBuffer, loading the buffer module on first use.
pub(crate) fn fast_buffer(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    if !shared.borrow().builtin_cache.contains_key(FAST_BUFFER_CACHE) {
        crate::host::shared_vm::commonjs::buffer_module(c)?;
    }
    let fast = shared.borrow().builtin_cache.get(FAST_BUFFER_CACHE).copied();
    fast.ok_or_else(|| RootedError::host("canonical FastBuffer missing"))
}

/// A new Buffer holding a copy of `bytes`. A class's `prototype` is non-writable and
/// non-configurable, so FastBuffer's is read rather than stored again.
pub(crate) fn from_bytes(c: &mut Context<'_>, bytes: &[u8]) -> Result<RootId, RootedError> {
    let fast = fast_buffer(c)?;
    let prototype = get(c, fast, "prototype")?;
    let value = c.uint8_array_rooted(bytes)?;
    if !c.set_prototype_rooted(value, prototype)? {
        return Err(RootedError::host("New Buffer view rejected its intrinsic prototype"));
    }
    Ok(value)
}

fn type_error(c: &mut Context<'_>, message: &str, code: &str) -> RootedError {
    let error = match c.type_error_rooted(message) {
        Ok(error) => error,
        Err(error) => return error,
    };
    let key = c.string_rooted("code");
    let value = c.string_rooted(code);
    if let Err(error) = c.set_property_rooted(error, key, value, error) {
        return error;
    }
    c.throw(error)
}

/// A string's bytes under a requested encoding (`Buffer.from(string, encoding)`'s policy):
/// undefined means UTF-8, and only Node's encoding names are accepted.
pub(crate) fn encode_bytes(c: &mut Context<'_>, input: RootId, encoding: Option<RootId>) -> Result<Vec<u8>, RootedError> {
    let Some(units) = c.string_code_units(input)? else {
        return Err(type_error(c, "The \"string\" argument must be of type string", "ERR_INVALID_ARG_TYPE"));
    };
    let name = match encoding.filter(|encoding| !c.rooted_value(*encoding).is_some_and(|value| value.is_undefined())) {
        None => "utf8".to_owned(),
        Some(encoding) => match c.string_text(encoding)? {
            Some(name) => name,
            None => return Err(type_error(c, "The \"encoding\" argument must be of type string", "ERR_INVALID_ARG_TYPE")),
        },
    };
    let Some(canonical) = super::buffer_codec::canonical_encoding(&name) else {
        return Err(type_error(c, &format!("Unknown encoding: {name}"), "ERR_UNKNOWN_ENCODING"));
    };
    Ok(super::buffer_codec::encode_units(&units, canonical))
}
