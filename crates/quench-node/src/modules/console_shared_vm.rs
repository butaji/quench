//! The configured Node SDK owns the Console constructor and global namespace.
//!
//! The SDK console loads inspect, errors, validators, buffer and the primordials, more than the
//! rest of a run usually allocates. The global `console` is therefore the object Node's
//! `internal/console/global.js` makes, built ahead of it: the same prototype shape, the SDK's
//! own keys in its order (`CONSOLE_KEYS`), and `Symbol.toStringTag`. Every key but `log` is a
//! lazy member that loads the SDK, whose own properties the console then adopts (`adopt`);
//! `log` stays a function of the SDK's name and length that writes a call of plain strings to
//! `process.stdout` as the SDK would until the SDK is loaded, and calls the SDK's `log`
//! otherwise. Listing the console's keys with its symbols loads the SDK first.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

/// The global console (`facade`).
const CONSOLE: &str = "\0node/console/global";
/// The SDK's own global console, once loaded; its methods are bound to it.
const SDK_CONSOLE: &str = "\0node/console/sdk";
/// Set while the SDK console's properties move to the global console.
const ADOPTING: &str = "\0node/console/adopting";
/// The streams the unloaded console wrote to, which the SDK console then keeps.
const STDOUT: &str = "\0node/console/stdout";
const STDERR: &str = "\0node/console/stderr";

/// The own string keys of the SDK's global console after Node's bootstrap, in order, with
/// whether each is enumerable. `log` comes first and stays the global console's own.
const CONSOLE_KEYS: [(&str, bool); 26] = [
    ("log", true),
    ("info", true),
    ("debug", true),
    ("warn", true),
    ("error", true),
    ("dir", true),
    ("time", true),
    ("timeEnd", true),
    ("timeLog", true),
    ("trace", true),
    ("assert", true),
    ("clear", true),
    ("count", true),
    ("countReset", true),
    ("group", true),
    ("groupEnd", true),
    ("table", true),
    ("dirxml", true),
    ("groupCollapsed", true),
    ("_stdoutErrorHandler", false),
    ("_stderrErrorHandler", false),
    ("_ignoreErrors", false),
    ("_times", false),
    ("Console", true),
    ("_stdout", false),
    ("_stderr", false),
];

/// The `%` that starts a format specifier in the SDK's `formatWithOptions`.
const FORMAT_SPECIFIER: char = '%';

fn cached(context: &mut NativeContext<'_, NodeHost>, name: &str) -> Option<RootId> {
    context.host_mut().shared_state().borrow().builtin_cache.get(name).copied()
}

fn cache(context: &mut NativeContext<'_, NodeHost>, name: &str, value: RootId) -> Result<(), RootedError> {
    let value = context.retain(value)?;
    context.host_mut().shared_state().borrow_mut().builtin_cache.insert(name.into(), value);
    Ok(())
}

/// The global console, made on first use.
pub(crate) fn facade(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    if let Some(console) = cached(context, CONSOLE) {
        return Ok(console);
    }
    // `{ __proto__: {} }`, as `internal/console/global.js` makes it.
    let console = context.object_rooted()?;
    let prototype = context.object_rooted()?;
    context.set_prototype_rooted(console, prototype)?;
    // Bound to the console, as the SDK binds every method of its global console.
    // Called directly: compiling a helper would fault the compiler back in after the entry's
    // compilation released it.
    let log = context.host_function(crate::host::shared_vm::operation("consoleLog"))?;
    let global = context.global_root()?;
    let function_key = context.string_rooted("Function");
    let function = context.get_property_rooted(global, function_key)?;
    let prototype_key = context.string_rooted("prototype");
    let function_prototype = context.get_property_rooted(function, prototype_key)?;
    let bind_key = context.string_rooted("bind");
    let bind = context.get_property_rooted(function_prototype, bind_key)?;
    let log = context.call_rooted(bind, log, &[console])?;
    let (log_key, enumerable) = CONSOLE_KEYS[0];
    let key = context.string_rooted(log_key);
    context.define_data_property_rooted(console, key, log, true, enumerable, true)?;
    let name = context.string_rooted("name");
    context.define_data_property_rooted(log, name, key, false, false, true)?;
    let length = context.string_rooted("length");
    let variadic = context.number(0.0);
    context.define_data_property_rooted(log, length, variadic, false, false, true)?;
    let initializer = context.host_function(crate::host::shared_vm::operation("consoleLoad"))?;
    context.define_lazy_holder_keys_rooted(console, &CONSOLE_KEYS[1..], initializer)?;
    let symbol_key = context.string_rooted("Symbol");
    let symbol = context.get_property_rooted(global, symbol_key)?;
    let tag_key = context.string_rooted("toStringTag");
    let tag = context.get_property_rooted(symbol, tag_key)?;
    let tag_value = context.string_rooted("console");
    context.define_data_property_rooted(console, tag, tag_value, false, false, true)?;
    cache(context, CONSOLE, console)?;
    Ok(console)
}

/// The lazy members' initializer: loads the SDK console, whose load adopts it (`adopt`).
pub(crate) fn load(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    if cached(context, ADOPTING).is_none() && cached(context, SDK_CONSOLE).is_none() {
        crate::host::shared_vm::commonjs::console_module(context)?;
    }
    Ok(context.undefined())
}

/// Called as `internal/console/global.js` finishes with the SDK's console `sdk`: the global
/// console takes its own properties, in its order, and becomes the module's export.
pub(crate) fn adopt(context: &mut NativeContext<'_, NodeHost>, module: RootId, sdk: RootId) -> Result<(), RootedError> {
    let console = facade(context)?;
    let marker = context.boolean(true);
    cache(context, ADOPTING, marker)?;
    cache(context, SDK_CONSOLE, sdk)?;
    let stdout = cached(context, STDOUT).unwrap_or_else(|| context.undefined());
    let stderr = cached(context, STDERR).unwrap_or_else(|| context.undefined());
    let adopt = context.evaluate_script_rooted(
        "(sdk, console, stdout, stderr) => {\n\
           for (const key of Reflect.ownKeys(sdk)) {\n\
             if (key === 'log') continue;\n\
             if (key === Symbol.toStringTag) delete console[key];\n\
             Reflect.defineProperty(console, key, Reflect.getOwnPropertyDescriptor(sdk, key));\n\
           }\n\
           if (stdout !== undefined) sdk._stdout = stdout;\n\
           if (stderr !== undefined) sdk._stderr = stderr;\n\
           return Reflect.ownKeys(sdk).filter((key) => typeof key === 'string');\n\
         }",
        "node:internal/console/adopt.js",
    )?;
    let undefined = context.undefined();
    let keys = context.call_rooted(adopt, undefined, &[sdk, console, stdout, stderr])?;
    debug_assert!(sdk_keys_match(context, keys), "CONSOLE_KEYS follows the SDK console's keys");
    let exports = context.string_rooted("exports");
    context.set_property_rooted(module, exports, console, module)?;
    Ok(())
}

/// Whether the SDK console's string keys `keys` are `CONSOLE_KEYS`.
fn sdk_keys_match(context: &mut NativeContext<'_, NodeHost>, keys: RootId) -> bool {
    let length_key = context.string_rooted("length");
    let Ok(length) = context.get_property_rooted(keys, length_key) else { return false };
    let Ok(length) = context.to_number(length) else { return false };
    length as usize == CONSOLE_KEYS.len()
        && CONSOLE_KEYS.iter().enumerate().all(|(index, (name, _))| {
            let index = context.string_rooted(&index.to_string());
            context
                .get_property_rooted(keys, index)
                .and_then(|key| context.string_text(key))
                .is_ok_and(|key| key.as_deref() == Some(*name))
        })
}

/// The global console's `log`.
pub(crate) fn log(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    crate::host::resident_pages::around_output(|| write_log(context, args))
}

fn write_log(context: &mut NativeContext<'_, NodeHost>, args: &[RootId]) -> Result<RootId, RootedError> {
    if cached(context, SDK_CONSOLE).is_none() {
        let mut parts = Vec::with_capacity(args.len());
        for &arg in args {
            match context.string_text(arg)? {
                Some(text) => parts.push(text),
                None => break,
            }
        }
        let plain = parts.len() == args.len()
            && parts.first().is_none_or(|first| !first.contains(FORMAT_SPECIFIER));
        if plain {
            return write_stream(context, STDOUT, "stdout", &(parts.join(" ") + "\n"));
        }
        let console = facade(context)?;
        context.materialize_lazy_data_property_rooted(console, CONSOLE_KEYS[1].0)?;
    }
    let sdk = cached(context, SDK_CONSOLE).ok_or_else(|| RootedError::host("SDK console did not load"))?;
    let key = context.string_rooted(CONSOLE_KEYS[0].0);
    let log = context.get_property_rooted(sdk, key)?;
    context.call_rooted(log, sdk, args)
}

/// Writes `text` to `process[name]`, the stream cached as `cache_key` once first written,
/// as the SDK console does, which ignores write errors.
fn write_stream(
    context: &mut NativeContext<'_, NodeHost>, cache_key: &str, name: &str, text: &str,
) -> Result<RootId, RootedError> {
    let stream = match cached(context, cache_key) {
        Some(stream) => stream,
        None => {
            let process = context
                .host_mut()
                .shared_state()
                .borrow()
                .process_module
                .ok_or_else(|| RootedError::host("process is not initialized"))?;
            let key = context.string_rooted(name);
            let stream = context.get_property_rooted(process, key)?;
            cache(context, cache_key, stream)?;
            stream
        }
    };
    let key = context.string_rooted("write");
    let write = context.get_property_rooted(stream, key)?;
    let text = context.string_rooted(text);
    let _ = context.call_rooted(write, stream, &[text]);
    Ok(context.undefined())
}

/// The console key Node's `writeOut` (`internal/process/warning.js`) writes warnings with.
const WARNING_KEY: &str = "error";

/// What Node's `writeOut` writes a warning with, read when it first writes one: the global
/// console's `error`. While that is still the SDK's pending member, a writer that writes a
/// line as the SDK's `error` writes one string (`warning_write`), so a warning does not
/// load the SDK.
pub(crate) fn warning_writer(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let console = facade(context)?;
    if context.is_pending_lazy_data_property_rooted(console, WARNING_KEY)? {
        return context.host_function(crate::host::shared_vm::operation("consoleWarningWrite"));
    }
    let key = context.string_rooted(WARNING_KEY);
    context.get_property_rooted(console, key)
}

/// The SDK's `error` with one string: the string and a newline on `process.stderr`, or the
/// SDK's own `error` once the SDK is loaded (its group indentation, its stream).
pub(crate) fn warning_write(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if let Some(sdk) = cached(context, SDK_CONSOLE) {
        let key = context.string_rooted(WARNING_KEY);
        let error = context.get_property_rooted(sdk, key)?;
        return context.call_rooted(error, sdk, args);
    }
    let undefined = context.undefined();
    let message = context.to_string(args.first().copied().unwrap_or(undefined))?;
    write_stream(context, STDERR, "stderr", &(message + "\n"))
}

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let console =
        crate::host::shared_vm::commonjs::node_library_module(context, "console.js", "console.js")?;
    let constructor_key = context.string_rooted("Console");
    let constructor = context.get_property_rooted(console, constructor_key)?;
    let prototype_key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(constructor, prototype_key)?;
    // Node's global console boundary is variadic and names aliases by their
    // namespace keys. Instance methods keep the SDK's original metadata.
    let name = context.string_rooted("name");
    let length = context.string_rooted("length");
    let variadic = context.number(0.0);
    for key in context.own_non_index_keys_rooted(prototype, true)? {
        if context.string_text(key)?.is_none() {
            continue;
        }
        let method = context.get_property_rooted(console, key)?;
        if !context.is_callable_rooted(method)? {
            continue;
        }
        context.define_data_property_rooted(method, name, key, false, false, true)?;
        context.define_data_property_rooted(method, length, variadic, false, false, true)?;
    }
    Ok(console)
}
