//! Node's process warnings: `process.emitWarning` and the default `warning` listener.

use quench_runtime::{NativeContext, RootId, RootedError};

use crate::host::shared_vm::commonjs;
use crate::host::NodeHost;


/// Node's warning module, relative to the library's `lib`.
const WARNING_MODULE: &str = "internal/process/warning.js";

#[cfg(test)]
mod scheduling_tests {
    #[test]
    fn warnings_are_deferred_and_keep_error_identity() {
        let source = r#"
          const jobs=[], delivered=[];
          process.nextTick=function(fn,...args){jobs.push([fn,args]);};
          process.emit=function(event,warning){delivered.push([event,warning]);return true;};
          if(process.emitWarning.name!=='emitWarning'||process.emitWarning.length!==4||!Object.hasOwn(process.emitWarning,'prototype'))throw Error('emitWarning metadata');
          process.emitWarning('hello',{type:'CustomWarning',code:'CODE',detail:'detail'});
          if(jobs.length!==1||delivered.length!==0)throw Error('warning was not deferred');
          const [job,args]=jobs.shift();
          if(job.name!=='doEmitWarning'||job.length!==1||args.length!==1)throw Error('tick callback metadata');
          const warning=args[0];
          if(warning.message!=='hello'||warning.name!=='CustomWarning'||warning.code!=='CODE'||warning.detail!=='detail')throw Error('warning fields');
          job(...args);
          if(delivered[0][0]!=='warning'||delivered[0][1]!==warning)throw Error('warning identity');
          process.noDeprecation=true;
          process.emitWarning(7,'DeprecationWarning');
          if(jobs.length)throw Error('suppressed warning was queued');
          process.noDeprecation=false;process.throwDeprecation=true;
          const original=new Error('original');original.name='DeprecationWarning';
          process.emitWarning(original);
          if(jobs.length!==1)throw Error('deprecation throw not deferred');
          try{jobs.shift()[0]();throw Error('missing throw');}catch(e){if(e!==original)throw e;}
        "#;
        let program = quench_runtime::Engine::specialize(source,"native-warning-scheduling.js").unwrap();
        let host = crate::NodeHost::new(vec!["quench-node".into()]);
        quench_runtime::Runtime::new(host).execute(&program).unwrap();
    }
}


/// The `[emitWarning, onWarning]` pair, built once.
fn binding(c: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    const BINDING: &str = "\0binding:process-warning";
    let shared = c.host_mut().shared_state();
    if let Some(pair) = shared.borrow().builtin_cache.get(BINDING).copied() { return Ok(pair); }
    let state = c.null_object_rooted()?;
    let process = shared.borrow().process_module.ok_or_else(|| RootedError::host("warning process missing"))?;
    c.assign_named_property_rooted(state, "process", process)?;
    let emit = c.host_constructor_named_with_data(crate::host::shared_vm::operation("warningEmit"), "emitWarning", state)?;
    let on = c.host_constructor_named_with_data(crate::host::shared_vm::operation("warningOn"), "onWarning", state)?;
    let pair = c.array_rooted(&[emit, on])?;
    let pair = c.retain(pair)?;
    shared.borrow_mut().builtin_cache.insert(BINDING.to_owned(), pair);
    Ok(pair)
}

use super::event_constructor_shared_vm::get;
use quench_runtime::ValueType;
type Context<'a> = NativeContext<'a, NodeHost>;

fn undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}
fn named(c: &mut Context<'_>, value: RootId, expected: &str) -> Result<bool, RootedError> {
    Ok(c.string_text(value)?.as_deref() == Some(expected))
}
fn validate_string(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.value_type_rooted(value) != Some(ValueType::String) {
        return Err(super::util_shared_vm::argument_type_error(c, value, name, "string")?);
    }
    Ok(())
}

pub(crate) fn emit(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let mut warning = args.first().copied().unwrap_or(missing);
    let mut kind = args.get(1).copied().unwrap_or(missing);
    let mut code = args.get(2).copied().unwrap_or(missing);
    let mut constructor = args.get(3).copied().unwrap_or(missing);
    let state = c.host_function_data()?;
    let process = get(c, state, "process")?;
    let no = get(c, process, "noDeprecation")?;
    if c.truthy_rooted(no)? && named(c, kind, "DeprecationWarning")? { return Ok(missing); }
    let mut detail = missing;
    if c.value_type_rooted(kind) == Some(ValueType::Object) && !c.is_array_rooted(kind)? {
        constructor = get(c, kind, "ctor")?;
        code = get(c, kind, "code")?;
        let value = get(c, kind, "detail")?;
        if c.value_type_rooted(value) == Some(ValueType::String) { detail = get(c, kind, "detail")?; }
        kind = get(c, kind, "type")?;
        if !c.truthy_rooted(kind)? { kind = c.string_rooted("Warning"); }
    } else if c.is_callable_rooted(kind)? {
        constructor = kind; code = missing; kind = c.string_rooted("Warning");
    }
    if !undefined(c, kind) { validate_string(c, kind, "type")?; }
    if c.is_callable_rooted(code)? { constructor = code; code = missing; }
    else if !undefined(c, code) { validate_string(c, code, "code")?; }
    let error_constructor = c.with_startup_intrinsics(|c| {
        let global = c.global_root()?; get(c, global, "Error")
    })?;
    if c.value_type_rooted(warning) == Some(ValueType::String) {
        warning = c.construct_rooted(error_constructor, error_constructor, &[warning])?;
        if !c.truthy_rooted(kind)? { kind = c.string_rooted("Warning"); }
        c.assign_named_property_rooted(warning, "name", kind)?;
        if !undefined(c, code) { c.assign_named_property_rooted(warning, "code", code)?; }
        if !undefined(c, detail) { c.assign_named_property_rooted(warning, "detail", detail)?; }
        let capture = get(c, error_constructor, "captureStackTrace")?;
        if !c.truthy_rooted(constructor)? { constructor = get(c, process, "emitWarning")?; }
        c.call_rooted(capture, error_constructor, &[warning, constructor])?;
    } else if !c.instance_of_rooted(warning, error_constructor)? {
        return Err(super::util_shared_vm::argument_type_error_with_expected(c, warning, "warning", "of type string or an instance of Error")?);
    }
    let name = get(c, warning, "name")?;
    let mut throw = false;
    if named(c, name, "DeprecationWarning")? {
        let no = get(c, process, "noDeprecation")?;
        if c.truthy_rooted(no)? { return Ok(missing); }
        let flag = get(c, process, "throwDeprecation")?;
        throw = c.truthy_rooted(flag)?;
    }
    let job = if throw {
        c.host_function_named_with_data(crate::host::shared_vm::operation("warningThrowTick"), "", warning)?
    } else {
        c.host_constructor_named_with_data(crate::host::shared_vm::operation("warningEmitTick"), "doEmitWarning", process)?
    };
    let tick = get(c, process, "nextTick")?;
    if throw { c.call_rooted(tick, process, &[job])?; }
    else { c.call_rooted(tick, process, &[job, warning])?; }
    Ok(missing)
}

pub(crate) fn emit_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let process = c.host_function_data()?;
    let warning = args.first().copied().unwrap_or_else(|| c.undefined());
    let emit = get(c, process, "emit")?;
    let event = c.string_rooted("warning");
    c.call_rooted(emit, process, &[event, warning])?;
    Ok(c.undefined())
}
pub(crate) fn throw_tick(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let warning = c.host_function_data()?;
    Err(c.throw(warning))
}

pub(crate) fn on(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let warning = args.first().copied().unwrap_or(missing);
    let state = c.host_function_data()?;
    let process = get(c, state, "process")?;
    let name = if undefined(c, warning) || c.rooted_value(warning).is_some_and(|value| value.is_null()) { missing } else { get(c, warning, "name")? };
    let code = if undefined(c, warning) || c.rooted_value(warning).is_some_and(|value| value.is_null()) { missing } else { get(c, warning, "code")? };
    let disabled = c.host_mut().shared_state().borrow().cli_options.repeated_values()
        .find(|(name, _)| *name == "--disable-warning").map(|(_, values)| values.to_vec()).unwrap_or_default();
    for value in disabled { if named(c, name, &value)? || named(c, code, &value)? { return Ok(missing); } }
    let error = c.with_startup_intrinsics(|c| { let global = c.global_root()?; get(c, global, "Error") })?;
    if !c.instance_of_rooted(warning, error)? { return Ok(missing); }
    let deprecation = named(c, name, "DeprecationWarning")?;
    let no = get(c, process, "noDeprecation")?;
    if deprecation && c.truthy_rooted(no)? { return Ok(missing); }
    let trace = get(c, process, "traceProcessWarnings")?;
    let trace = if c.truthy_rooted(trace)? { true } else if deprecation {
        let value = get(c, process, "traceDeprecation")?; c.truthy_rooted(value)?
    } else { false };
    let release = get(c, process, "release")?;
    let release = get(c, release, "name")?;
    let release = c.to_string(release)?;
    let pid = get(c, process, "pid")?;
    let pid = c.to_string(pid)?;
    let mut text: Vec<u16> = format!("({release}:{pid}) ").encode_utf16().collect();
    if c.truthy_rooted(code)? { text.push(u16::from(b'[')); text.extend(c.to_string_code_units(code)?); text.extend("] ".encode_utf16()); }
    let stack = if trace { get(c, warning, "stack")? } else { missing };
    let rendered = if trace && c.truthy_rooted(stack)? { stack } else {
        let to_string = get(c, warning, "toString")?;
        if c.is_callable_rooted(to_string)? { c.call_rooted(to_string, warning, &[])? } else {
            let prototype = get(c, error, "prototype")?;
            let to_string = get(c, prototype, "toString")?;
            c.call_rooted(to_string, warning, &[])?
        }
    };
    text.extend(c.to_string_code_units(rendered)?);
    let detail = get(c, warning, "detail")?;
    if c.value_type_rooted(detail) == Some(ValueType::String) { text.push(u16::from(b'\n')); text.extend(c.to_string_code_units(detail)?); }
    if !trace && !c.has_own_property_rooted(state, "hintShown")? {
        let argv = get(c, process, "argv0")?;
        let argv = if c.truthy_rooted(argv)? { c.to_string(argv)? } else { "node".to_owned() };
        let flag = if deprecation { "--trace-deprecation" } else { "--trace-warnings" };
        text.extend(format!("\n(Use `{} {flag} ...` to show where the warning was created)", basename(&argv, ".exe")).encode_utf16());
        let yes = c.boolean(true); c.assign_named_property_rooted(state, "hintShown", yes)?;
    }
    let message = c.string_units_rooted(&text);
    let mut writer = get(c, state, "writer")?;
    if undefined(c, writer) { writer = super::console_shared_vm::warning_writer(c, missing, &[])?; c.assign_named_property_rooted(state, "writer", writer)?; }
    c.call_rooted(writer, missing, &[message])?;
    Ok(missing)
}

/// Builds the functions ahead of a warning that follows a release of the compiler's pages
/// (`esm_wasm`: compiling the Wasm module releases them, and its warning comes next).
pub(crate) fn prepare(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    binding(context).map(drop)
}

fn member(context: &mut NativeContext<'_, NodeHost>, index: usize) -> Result<RootId, RootedError> {
    let pair = binding(context)?;
    let key = context.string_rooted(&index.to_string());
    context.get_property_rooted(pair, key)
}

/// `process.emitWarning`: Node's `emitWarning`.
pub(crate) fn emit_warning(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    member(context, 0)
}

/// The default `warning` listener's implementation: Node's `onWarning`.
pub(crate) fn on_warning(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    member(context, 1)
}

fn string_argument(context: &mut NativeContext<'_, NodeHost>, args: &[RootId], index: usize) -> Result<String, RootedError> {
    let undefined = context.undefined();
    context.to_string(args.get(index).copied().unwrap_or(undefined))
}

/// `library(id)`: Node's library module `id`, loaded as the warning module would require it.
pub(crate) fn library(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = string_argument(context, args, 0)?;
    commonjs::node_library_module(context, WARNING_MODULE, &format!("{id}.js"))
}

/// `getOptionValue(name)` for the options the module reads.
pub(crate) fn option(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let name = string_argument(context, args, 0)?;
    let (string, repeated) = {
        let state = context.host_mut().shared_state();
        let state = state.borrow();
        let options = &state.cli_options;
        let repeated = options.repeated_values().find(|(option, _)| *option == name).map(|(_, values)| values.to_vec());
        (options.string_value(&name).map(str::to_owned), repeated)
    };
    match (string, repeated) {
        (Some(value), _) => Ok(context.string_rooted(&value)),
        (None, Some(values)) => {
            let values: Vec<RootId> = values.iter().map(|value| context.string_rooted(value)).collect();
            context.array_rooted(&values)
        }
        (None, None) => Err(RootedError::host(format!("the warning module reads no option {name}"))),
    }
}

/// `path.posix.basename(path, suffix)` (`lib/path.js`) over UTF-8 text. The module passes
/// an ASCII suffix (`'.exe'`), which matches byte by byte from the end exactly where it
/// matches code unit by code unit.
fn basename<'a>(path: &'a str, suffix: &str) -> &'a str {
    let bytes = path.as_bytes();
    let suffix = suffix.as_bytes();
    let separator = b'/';
    let mut start = 0;
    let mut matched_slash = true;
    if !suffix.is_empty() && suffix.len() <= bytes.len() {
        if suffix == bytes {
            return "";
        }
        let mut suffix_index = Some(suffix.len() - 1);
        let mut end = None;
        let mut first_non_slash_end = None;
        for index in (0..bytes.len()).rev() {
            let byte = bytes[index];
            if byte == separator {
                if !matched_slash {
                    start = index + 1;
                    break;
                }
                continue;
            }
            if first_non_slash_end.is_none() {
                matched_slash = false;
                first_non_slash_end = Some(index + 1);
            }
            if let Some(position) = suffix_index {
                if byte == suffix[position] {
                    suffix_index = position.checked_sub(1);
                    if suffix_index.is_none() {
                        end = Some(index);
                    }
                } else {
                    suffix_index = None;
                    end = first_non_slash_end;
                }
            }
        }
        let end = match end {
            Some(end) if end == start => first_non_slash_end,
            Some(end) => Some(end),
            None => Some(bytes.len()),
        };
        return end.map_or("", |end| &path[start..end]);
    }
    let mut end = None;
    for index in (0..bytes.len()).rev() {
        if bytes[index] == separator {
            if !matched_slash {
                start = index + 1;
                break;
            }
        } else if end.is_none() {
            matched_slash = false;
            end = Some(index + 1);
        }
    }
    end.map_or("", |end| &path[start..end])
}

/// `require('path').basename(path, suffix)`.
pub(crate) fn path_basename(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let path = string_argument(context, args, 0)?;
    let suffix = string_argument(context, args, 1)?;
    Ok(context.string_rooted(basename(&path, &suffix)))
}

#[cfg(test)]
mod tests {
    use super::basename;

    #[test]
    fn basename_is_node_posix_basename() {
        for (path, expected) in [
            ("/usr/bin/node", "node"),
            ("node", "node"),
            ("/opt/node.exe", "node"),
            ("node.exe/", "node"),
            ("/a/b/", "b"),
            (".exe", ""),
            ("x.exe.exe", "x.exe"),
            ("/", ""),
            ("", ""),
            ("a/.exe", ".exe"),
            ("quench-node", "quench-node"),
        ] {
            assert_eq!(basename(path, ".exe"), expected, "{path}");
        }
    }
}
