//! CLI eval's lazy builtin globals, using the original Module loader and VM properties.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};
type Context<'a> = NativeContext<'a, NodeHost>;

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
fn named(c: &mut Context<'_>, function: RootId, name: &str) -> Result<(), RootedError> {
    let key = c.string_rooted("name");
    let value = c.string_rooted(name);
    c.define_data_property_rooted(function, key, value, false, false, true)?;
    Ok(())
}
fn descriptor(c: &mut Context<'_>, getter: RootId, setter: RootId) -> Result<RootId, RootedError> {
    let descriptor = c.null_object_rooted()?;
    put(c, descriptor, "get", getter)?;
    put(c, descriptor, "set", setter)?;
    let no = c.boolean(false);
    put(c, descriptor, "enumerable", no)?;
    let yes = c.boolean(true);
    put(c, descriptor, "configurable", yes)?;
    Ok(descriptor)
}

pub(crate) fn install(c: &mut Context<'_>) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let module = crate::host::shared_vm::commonjs::module_module(c)?;
    let id = c.string_rooted("<eval>");
    let dummy = c.construct_rooted(module, module, &[id])?;
    let delete = c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let reflect = get(c, global, "Reflect")?;
        get(c, reflect, "deleteProperty")
    })?;
    let vfs = c.host_mut().vfs_enabled();
    for name in super::module_shared_vm::BUILTIN_MODULES
        .iter()
        .copied()
        .chain(vfs.then_some("node:vfs"))
    {
        if name.starts_with('_') || name.contains('/') {
            continue;
        }
        let key = c.string_rooted(name);
        let own = c.own_property_descriptor_rooted(global, key)?;
        if !c.rooted_value(own).is_some_and(Value::is_undefined) {
            continue;
        }
        // Setter data does not retain the dummy module after the initial getter
        // is replaced. Saved initial getters retain their original loader.
        let binding = c.null_object_rooted()?;
        put(c, binding, "target", global)?;
        put(c, binding, "key", key)?;
        put(c, binding, "delete", delete)?;
        let setter = c.host_function_with_data(
            crate::host::shared_vm::operation("nodeEvalBuiltinSet"),
            binding,
        )?;
        named(c, setter, "setReal")?;
        put(c, binding, "setter", setter)?;
        let data = c.null_object_rooted()?;
        put(c, data, "binding", binding)?;
        put(c, data, "module", dummy)?;
        let getter = c.host_function_with_data(
            crate::host::shared_vm::operation("nodeEvalBuiltinGet"),
            data,
        )?;
        named(c, getter, "get")?;
        let descriptor = descriptor(c, getter, setter)?;
        c.define_property_descriptor_rooted(global, key, descriptor)?;
    }
    Ok(())
}

pub(crate) fn load(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let binding = get(c, data, "binding")?;
    let module = get(c, data, "module")?;
    let key = get(c, binding, "key")?;
    let require = get(c, module, "require")?;
    let value = c.call_rooted(require, module, &[key])?;
    let getter = c.host_function_with_data(
        crate::host::shared_vm::operation("nodeEvalBuiltinCachedGet"),
        value,
    )?;
    named(c, getter, "get")?;
    let setter = get(c, binding, "setter")?;
    let target = get(c, binding, "target")?;
    let descriptor = descriptor(c, getter, setter)?;
    // Node returns the loaded value even if user code made the property
    // nonconfigurable while the initial getter was still installed.
    if let Err(error) = c.define_property_descriptor_rooted(target, key, descriptor) {
        if let Some(exception) = error.exception {
            c.release_root(exception);
        }
    }
    Ok(value)
}

pub(crate) fn cached(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    c.host_function_data()
}

pub(crate) fn replace(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let binding = c.host_function_data()?;
    let target = get(c, binding, "target")?;
    let key = get(c, binding, "key")?;
    let delete = get(c, binding, "delete")?;
    let receiver = c.undefined();
    let deleted = c.call_rooted(delete, receiver, &[target, key])?;
    if !c.truthy_rooted(deleted)? {
        let name = c.to_string(key)?;
        let error =
            c.type_error_rooted(&format!("Cannot delete property '{name}' of #<Object>"))?;
        return Err(c.throw(error));
    }
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    if !c.set_property_rooted(target, key, value, target)? {
        let name = c.to_string(key)?;
        let error = c.type_error_rooted(&format!(
            "Cannot assign to read only property '{name}' of object '#<Object>'"
        ))?;
        return Err(c.throw(error));
    }
    Ok(c.undefined())
}

const CRYPTO: &str = "crypto";

/// Whether CommonJS eval source has its `crypto` bound to `node:crypto` while
/// `globalThis.crypto` stays WebCrypto: Node binds it around source naming `crypto` as a
/// word (`/\bcrypto\b/`), and source declaring its own top-level `crypto` shadows that.
pub(crate) fn binds_crypto(source: &str) -> bool {
    let word = |character: Option<char>| character.is_some_and(|character| character.is_ascii_alphanumeric() || character == '_');
    let names = source.match_indices(CRYPTO).any(|(index, _)| {
        !word(source[..index].chars().next_back()) && !word(source[index + CRYPTO.len()..].chars().next())
    });
    names && {
        let allocator = oxc_allocator::Allocator::default();
        let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::cjs()).parse();
        let semantic = oxc_semantic::SemanticBuilder::new().build(&parsed.program);
        semantic.semantic.scoping().get_root_binding(CRYPTO.into()).is_none()
    }
}

/// Binds the eval script's `crypto` to `node:crypto`, as Node's `let crypto =
/// require("node:crypto")` ahead of the eval source does.
#[cfg(quench_openssl_provider)]
pub(crate) fn bind_crypto(c: &mut Context<'_>) -> Result<(), RootedError> {
    let crypto = crate::host::shared_vm::commonjs::crypto_module(c)?;
    c.define_global_lexical_rooted(CRYPTO, crypto)
}

/// `--print`: evaluates the eval source as a Script and logs its completion value when the
/// process exits, as Node's `evalScript` does.
pub(crate) fn print(c: &mut Context<'_>, source: &str, name: &str) -> Result<(), RootedError> {
    let result = c.evaluate_script_rooted(source, name)?;
    let log = c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let console = get(c, global, "console")?;
        get(c, console, "log")
    })?;
    let data = c.array_rooted(&[log, result])?;
    let listener = c.host_function_with_data(crate::host::shared_vm::operation("nodePrintEvalResult"), data)?;
    let process = c
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    let on = get(c, process, "on")?;
    let exit = c.string_rooted("exit");
    c.call_rooted(on, process, &[exit, listener])?;
    Ok(())
}

/// The `--print` exit listener: `log(result)`.
pub(crate) fn print_result(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let log = get(c, data, "0")?;
    let result = get(c, data, "1")?;
    let undefined = c.undefined();
    c.call_rooted(log, undefined, &[result])?;
    Ok(undefined)
}
