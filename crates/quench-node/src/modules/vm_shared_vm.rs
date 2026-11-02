//! `node:vm` (lib/vm.js): `Script` over the binding's `ContextifyScript`, contexts made by
//! `createContext`, the `run*` conveniences, `compileFunction` and `measureMemory`.
//!
//! The binding (`contextify_shared_vm`) owns compiled code and contexts; this layer validates
//! Node's options and maps them onto the binding's positional arguments.
use super::contextify_shared_vm as contextify;
use super::event_constructor_shared_vm::get;
use super::util_shared_vm::{argument_type_error, argument_type_error_with_expected, argument_value_error};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, ValueType};

type Context<'a> = NativeContext<'a, NodeHost>;

const DEFAULT_FILENAME: &str = "evalmachine.<anonymous>";
const DEFAULT_FUNCTION_FILENAME: &str = "";
const CONTEXT_NAME_PREFIX: &str = "VM Context ";
/// `ContextifyScript.prototype.runInContext`'s timeout when none is given.
const NO_TIMEOUT: f64 = -1.0;
const BUFFER_SOURCE: &str = "an instance of Buffer, TypedArray, or DataView";
const MICROTASK_MODE_AFTER_EVALUATE: &str = "afterEvaluate";
const MEASURE_MEMORY_WARNING: &str = "vm.measureMemory is an experimental feature and might change at any time";

/// `vm.constants`: each symbol's description, in Node's order.
mod constant {
    pub(super) const USE_MAIN_CONTEXT_DEFAULT_LOADER: (&str, &str) =
        ("USE_MAIN_CONTEXT_DEFAULT_LOADER", "vm_dynamic_import_main_context_default");
    pub(super) const DONT_CONTEXTIFY: (&str, &str) = ("DONT_CONTEXTIFY", "vm_context_no_contextify");
}

/// `measureMemory`'s modes and executions, in Node's order; the first is the default.
const MEASURE_MODES: [&str; 2] = ["summary", "detailed"];
const MEASURE_EXECUTIONS: [&str; 2] = ["default", "eager"];
const EAGER_EXECUTION: &str = "eager";
const DETAILED_MODE: &str = "detailed";

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn kind(c: &Context<'_>, value: RootId) -> ValueType {
    c.value_type_rooted(value).unwrap_or(ValueType::Undefined)
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    kind(c, value) == ValueType::Undefined
}

/// `validateObject(value, name)`: an object, neither null, an array nor a function.
fn validate_object(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if kind(c, value) != ValueType::Object || c.is_array_rooted(value)? {
        return Err(argument_type_error(c, value, name, "Object")?);
    }
    Ok(())
}

fn validate_string(c: &mut Context<'_>, value: RootId, name: &str) -> Result<String, RootedError> {
    match c.string_text(value)? {
        Some(text) => Ok(text),
        None => Err(argument_type_error(c, value, name, "string")?),
    }
}

fn validate_boolean(c: &mut Context<'_>, value: RootId, name: &str) -> Result<bool, RootedError> {
    match c.rooted_value(value).and_then(|value| value.as_bool()) {
        Some(flag) => Ok(flag),
        None => Err(argument_type_error(c, value, name, "boolean")?),
    }
}

fn validate_int32(c: &mut Context<'_>, value: RootId, name: &str) -> Result<f64, RootedError> {
    let maximum = c.number(f64::from(i32::MAX));
    super::util_shared_vm::validate_integer(c, value, name, f64::from(i32::MIN), maximum)
}

/// `validateBuffer`: an ArrayBuffer view.
fn validate_buffer_source(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let array_buffer = get(c, global, "ArrayBuffer")?;
    let is_view = get(c, array_buffer, "isView")?;
    let view = c.call_rooted(is_view, array_buffer, &[value])?;
    if !c.truthy_rooted(view)? {
        return Err(argument_type_error_with_expected(c, value, name, BUFFER_SOURCE)?);
    }
    Ok(())
}

/// `validateOneOf(value, name, choices)` over string choices.
fn validate_one_of<'a>(c: &mut Context<'_>, value: RootId, name: &str, choices: &[&'a str]) -> Result<&'a str, RootedError> {
    let text = c.string_text(value)?;
    if let Some(choice) = text.and_then(|text| choices.iter().copied().find(|choice| *choice == text)) {
        return Ok(choice);
    }
    let listed = choices.iter().map(|choice| format!("'{choice}'")).collect::<Vec<_>>().join(", ");
    Err(argument_value_error(c, value, name, &format!("must be one of: {listed}"))?)
}

/// An `options` argument: absent when undefined, otherwise an object.
fn options_object(c: &mut Context<'_>, options: RootId) -> Result<Option<RootId>, RootedError> {
    if is_undefined(c, options) {
        return Ok(None);
    }
    validate_object(c, options, "options")?;
    Ok(Some(options))
}

/// `options[name]`; undefined without options.
fn option(c: &mut Context<'_>, options: Option<RootId>, name: &str) -> Result<RootId, RootedError> {
    match options {
        Some(options) => get(c, options, name),
        None => Ok(c.undefined()),
    }
}

/// `options[name]`, or `default` when it is undefined.
fn option_or(c: &mut Context<'_>, options: Option<RootId>, name: &str, default: RootId) -> Result<RootId, RootedError> {
    let value = option(c, options, name)?;
    Ok(if is_undefined(c, value) { default } else { value })
}

/// `importModuleDynamically`: a function or `vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER`.
fn validate_import_callback(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if is_undefined(c, value) || c.is_callable_rooted(value)? {
        return Ok(());
    }
    let loader = constant_symbol(c, constant::USE_MAIN_CONTEXT_DEFAULT_LOADER.0)?;
    if c.strict_equal_rooted(value, loader)? {
        return Ok(());
    }
    Err(argument_type_error(c, value, name, "function")?)
}

fn constants(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0builtin:vm-constants", |c| {
        let constants = c.null_object_rooted()?;
        for (name, description) in [constant::USE_MAIN_CONTEXT_DEFAULT_LOADER, constant::DONT_CONTEXTIFY] {
            let symbol = c.symbol_rooted(Some(description));
            let key = c.string_rooted(name);
            c.define_data_property_rooted(constants, key, symbol, false, true, false)?;
        }
        c.prevent_extensions_rooted(constants)?;
        Ok(constants)
    })
}

fn constant_symbol(c: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    let constants = constants(c)?;
    get(c, constants, name)
}

fn script_class(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0builtin:vm-script", |c| {
        let base = contextify::script_class(c)?;
        let base_prototype = get(c, base, "prototype")?;
        let class = c.host_constructor_named(crate::host::shared_vm::operation("vmScript"), "Script")?;
        c.set_prototype_rooted(class, base)?;
        let prototype = get(c, class, "prototype")?;
        c.set_prototype_rooted(prototype, base_prototype)?;
        // A class's prototype binding is fixed.
        let key = c.string_rooted("prototype");
        c.define_data_property_rooted(class, key, prototype, false, false, false)?;
        for (name, operation) in [
            ("runInThisContext", "vmScriptRunInThisContext"),
            ("runInContext", "vmScriptRunInContext"),
            ("runInNewContext", "vmScriptRunInNewContext"),
        ] {
            let method = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(prototype, key, method, true, false, true)?;
        }
        Ok(class)
    })
}

/// The `options` a `Script` and the `run*` functions accept: a filename string stands for
/// `{ filename }`.
fn script_options(c: &mut Context<'_>, options: RootId) -> Result<Option<RootId>, RootedError> {
    if kind(c, options) == ValueType::String {
        let wrapped = c.object_rooted()?;
        c.assign_named_property_rooted(wrapped, "filename", options)?;
        return Ok(Some(wrapped));
    }
    options_object(c, options)
}

/// `new Script(code, options)`.
pub(crate) fn script(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if c.new_target_rooted().is_none() {
        let error = c.type_error_rooted("Class constructor Script cannot be invoked without 'new'")?;
        return Err(c.throw(error));
    }
    let code = argument(c, args, 0);
    let code = c.to_string(code)?;
    let code = c.string_rooted(&code);
    let options = argument(c, args, 1);
    let options = script_options(c, options)?;
    let default_filename = c.string_rooted(DEFAULT_FILENAME);
    let filename = option_or(c, options, "filename", default_filename)?;
    let zero = c.number(0.0);
    let line = option_or(c, options, "lineOffset", zero)?;
    let column = option_or(c, options, "columnOffset", zero)?;
    let cached = option(c, options, "cachedData")?;
    let no = c.boolean(false);
    let produce = option_or(c, options, "produceCachedData", no)?;
    let import = option(c, options, "importModuleDynamically")?;
    validate_string(c, filename, "options.filename")?;
    validate_int32(c, line, "options.lineOffset")?;
    validate_int32(c, column, "options.columnOffset")?;
    if !is_undefined(c, cached) {
        validate_buffer_source(c, cached, "options.cachedData")?;
    }
    validate_boolean(c, produce, "options.produceCachedData")?;
    validate_import_callback(c, import, "options.importModuleDynamically")?;
    let undefined = c.undefined();
    contextify::script(c, receiver, &[code, filename, line, column, cached, produce, undefined])
}

/// `getRunInContextArgs(contextifiedObject, options)`: the binding's arguments.
fn run_arguments(c: &mut Context<'_>, context: RootId, options: RootId) -> Result<[RootId; 5], RootedError> {
    let options = options_object(c, options)?;
    let timeout = option(c, options, "timeout")?;
    let timeout = if is_undefined(c, timeout) {
        c.number(NO_TIMEOUT)
    } else {
        let maximum = c.number(f64::from(u32::MAX));
        super::util_shared_vm::validate_integer(c, timeout, "options.timeout", 1.0, maximum)?;
        timeout
    };
    let yes = c.boolean(true);
    let no = c.boolean(false);
    let display_errors = option_or(c, options, "displayErrors", yes)?;
    let break_on_sigint = option_or(c, options, "breakOnSigint", no)?;
    validate_boolean(c, display_errors, "options.displayErrors")?;
    validate_boolean(c, break_on_sigint, "options.breakOnSigint")?;
    Ok([context, timeout, display_errors, break_on_sigint, no])
}

/// `script.runInThisContext(options)`.
pub(crate) fn script_run_in_this_context(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let options = argument(c, args, 0);
    let null = c.null();
    let arguments = run_arguments(c, null, options)?;
    contextify::run_in_context(c, receiver, &arguments)
}

/// `script.runInContext(contextifiedObject, options)`.
pub(crate) fn script_run_in_context(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let context = argument(c, args, 0);
    let options = argument(c, args, 1);
    contextify::context_record(c, context)?;
    let arguments = run_arguments(c, context, options)?;
    contextify::run_in_context(c, receiver, &arguments)
}

/// `script.runInNewContext(contextObject, options)`.
pub(crate) fn script_run_in_new_context(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let sandbox = argument(c, args, 0);
    let options = argument(c, args, 1);
    let context = new_context(c, sandbox, options)?;
    script_run_in_context(c, receiver, &[context, options])
}

/// What a new context is made with.
struct ContextOptions {
    name: Option<RootId>,
    origin: RootId,
    strings: bool,
    wasm: bool,
    own_microtask_queue: bool,
    import: RootId,
}

/// `{ strings, wasm }` of a `codeGeneration` option, each true unless given.
fn code_generation(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(bool, bool), RootedError> {
    if is_undefined(c, value) {
        return Ok((true, true));
    }
    validate_object(c, value, name)?;
    let mut flags = [true; 2];
    for (flag, member) in flags.iter_mut().zip(["strings", "wasm"]) {
        let given = get(c, value, member)?;
        if !is_undefined(c, given) {
            *flag = validate_boolean(c, given, &format!("{name}.{member}"))?;
        }
    }
    Ok((flags[0], flags[1]))
}

fn microtask_mode(c: &mut Context<'_>, value: RootId, name: &str) -> Result<bool, RootedError> {
    if is_undefined(c, value) {
        return Ok(false);
    }
    if c.string_text(value)?.as_deref() == Some(MICROTASK_MODE_AFTER_EVALUATE) {
        return Ok(true);
    }
    let reason = format!("must be one of: '{MICROTASK_MODE_AFTER_EVALUATE}', undefined");
    Err(argument_value_error(c, value, name, &reason)?)
}

/// `isContext(object)`'s validation: any object, arrays and functions included.
fn validate_context_object(c: &mut Context<'_>, value: RootId) -> Result<(), RootedError> {
    if !c.is_object_rooted(value)? {
        return Err(argument_type_error(c, value, "object", "Object")?);
    }
    Ok(())
}

/// `sandbox` when it already is a context; any other value but `DONT_CONTEXTIFY` must be
/// an object.
fn existing_context(c: &mut Context<'_>, sandbox: RootId) -> Result<Option<RootId>, RootedError> {
    let no_contextify = constant_symbol(c, constant::DONT_CONTEXTIFY.0)?;
    if c.strict_equal_rooted(sandbox, no_contextify)? {
        return Ok(None);
    }
    validate_context_object(c, sandbox)?;
    Ok(contextify::find_record(c, sandbox)?.map(|_| sandbox))
}

/// `createContext(contextObject, options)` once its options are read.
fn make_context(c: &mut Context<'_>, sandbox: RootId, options: ContextOptions) -> Result<RootId, RootedError> {
    if let Some(context) = existing_context(c, sandbox)? {
        return Ok(context);
    }
    let name = match options.name {
        Some(name) => name,
        None => {
            let index = {
                let shared = c.host_mut().shared_state();
                let mut shared = shared.borrow_mut();
                let index = shared.vm_scripts.next_context_name;
                shared.vm_scripts.next_context_name += 1;
                index
            };
            c.string_rooted(&format!("{CONTEXT_NAME_PREFIX}{index}"))
        }
    };
    let [strings, wasm, queue] = [options.strings, options.wasm, options.own_microtask_queue].map(|flag| c.boolean(flag));
    let undefined = c.undefined();
    contextify::make_context(c, undefined, &[sandbox, name, options.origin, strings, wasm, queue, options.import])
}

/// `vm.createContext(contextObject = {}, options = {})`.
pub(crate) fn create_context(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let sandbox = argument(c, args, 0);
    let sandbox = if is_undefined(c, sandbox) { c.object_rooted()? } else { sandbox };
    if let Some(context) = existing_context(c, sandbox)? {
        return Ok(context);
    }
    let options = argument(c, args, 1);
    let options = options_object(c, options)?;
    let name = option(c, options, "name")?;
    let name = if is_undefined(c, name) {
        None
    } else {
        validate_string(c, name, "options.name")?;
        Some(name)
    };
    let origin = option(c, options, "origin")?;
    if !is_undefined(c, origin) {
        validate_string(c, origin, "options.origin")?;
    }
    let generation = option(c, options, "codeGeneration")?;
    let (strings, wasm) = code_generation(c, generation, "options.codeGeneration")?;
    let mode = option(c, options, "microtaskMode")?;
    let own_microtask_queue = microtask_mode(c, mode, "options.microtaskMode")?;
    let import = option(c, options, "importModuleDynamically")?;
    validate_import_callback(c, import, "options.importModuleDynamically")?;
    make_context(c, sandbox, ContextOptions { name, origin, strings, wasm, own_microtask_queue, import })
}

/// `getContextOptions(options)` applied: the context `runInNewContext` makes for `sandbox`.
fn new_context(c: &mut Context<'_>, sandbox: RootId, options: RootId) -> Result<RootId, RootedError> {
    let sandbox = if is_undefined(c, sandbox) { c.object_rooted()? } else { sandbox };
    let options = if is_undefined(c, options) || kind(c, options) != ValueType::Object { None } else { Some(options) };
    let name = option(c, options, "contextName")?;
    let name = if is_undefined(c, name) {
        None
    } else {
        validate_string(c, name, "options.contextName")?;
        Some(name)
    };
    let origin = option(c, options, "contextOrigin")?;
    if !is_undefined(c, origin) {
        validate_string(c, origin, "options.contextOrigin")?;
    }
    let generation = option(c, options, "contextCodeGeneration")?;
    let (strings, wasm) = code_generation(c, generation, "options.contextCodeGeneration")?;
    let mode = option(c, options, "microtaskMode")?;
    if !is_undefined(c, mode) {
        validate_string(c, mode, "options.microtaskMode")?;
    }
    let own_microtask_queue = microtask_mode(c, mode, "options.microtaskMode")?;
    let import = c.undefined();
    make_context(c, sandbox, ContextOptions { name, origin, strings, wasm, own_microtask_queue, import })
}

/// `vm.isContext(object)`.
pub(crate) fn is_context(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let object = argument(c, args, 0);
    validate_context_object(c, object)?;
    contextify::is_context(c, receiver, args)
}

fn new_script(c: &mut Context<'_>, code: RootId, options: RootId) -> Result<RootId, RootedError> {
    let class = script_class(c)?;
    c.construct_rooted(class, class, &[code, options])
}

/// `vm.createScript(code, options)`.
pub(crate) fn create_script(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let code = argument(c, args, 0);
    let options = argument(c, args, 1);
    new_script(c, code, options)
}

/// The run options of a `run*` call: a filename string runs with default options.
fn run_options(c: &mut Context<'_>, options: RootId) -> RootId {
    if kind(c, options) == ValueType::String { c.undefined() } else { options }
}

/// `vm.runInThisContext(code, options)`.
pub(crate) fn run_in_this_context(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let code = argument(c, args, 0);
    let options = argument(c, args, 1);
    let script = new_script(c, code, options)?;
    let options = run_options(c, options);
    script_run_in_this_context(c, script, &[options])
}

/// `vm.runInContext(code, contextifiedObject, options)`.
pub(crate) fn run_in_context(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let code = argument(c, args, 0);
    let context = argument(c, args, 1);
    let options = argument(c, args, 2);
    contextify::context_record(c, context)?;
    let script = new_script(c, code, options)?;
    let options = run_options(c, options);
    script_run_in_context(c, script, &[context, options])
}

/// `vm.runInNewContext(code, contextObject, options)`.
pub(crate) fn run_in_new_context(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let code = argument(c, args, 0);
    let sandbox = argument(c, args, 1);
    let options = argument(c, args, 2);
    let context = new_context(c, sandbox, options)?;
    let script = new_script(c, code, options)?;
    let options = run_options(c, options);
    script_run_in_context(c, script, &[context, options])
}

/// `vm.compileFunction(code, params, options)`.
pub(crate) fn compile_function(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let code = argument(c, args, 0);
    validate_string(c, code, "code")?;
    let params = argument(c, args, 1);
    let params = if is_undefined(c, params) { c.array_rooted(&[])? } else { params };
    if !c.is_array_rooted(params)? {
        return Err(argument_type_error(c, params, "params", "Array")?);
    }
    for (index, param) in c.array_like_values_rooted(params)?.into_iter().enumerate() {
        validate_string(c, param, &format!("params[{index}]"))?;
    }
    let options = argument(c, args, 2);
    let options = options_object(c, options)?;
    let default_filename = c.string_rooted(DEFAULT_FUNCTION_FILENAME);
    let filename = option_or(c, options, "filename", default_filename)?;
    let zero = c.number(0.0);
    let column = option_or(c, options, "columnOffset", zero)?;
    let line = option_or(c, options, "lineOffset", zero)?;
    let cached = option(c, options, "cachedData")?;
    let no = c.boolean(false);
    let produce = option_or(c, options, "produceCachedData", no)?;
    let parsing_context = option(c, options, "parsingContext")?;
    let no_extensions = c.array_rooted(&[])?;
    let extensions = option_or(c, options, "contextExtensions", no_extensions)?;
    let import = option(c, options, "importModuleDynamically")?;
    validate_string(c, filename, "options.filename")?;
    validate_int32(c, column, "options.columnOffset")?;
    validate_int32(c, line, "options.lineOffset")?;
    if !is_undefined(c, cached) {
        validate_buffer_source(c, cached, "options.cachedData")?;
    }
    validate_boolean(c, produce, "options.produceCachedData")?;
    if !is_undefined(c, parsing_context) && contextify::find_record(c, parsing_context)?.is_none() {
        return Err(argument_type_error(c, parsing_context, "options.parsingContext", "Context")?);
    }
    if !c.is_array_rooted(extensions)? {
        return Err(argument_type_error(c, extensions, "options.contextExtensions", "Array")?);
    }
    for (index, extension) in c.array_like_values_rooted(extensions)?.into_iter().enumerate() {
        if kind(c, extension) != ValueType::Object && kind(c, extension) != ValueType::Null {
            return Err(argument_type_error(c, extension, &format!("options.contextExtensions[{index}]"), "Object")?);
        }
    }
    validate_import_callback(c, import, "options.importModuleDynamically")?;
    let result = contextify::compile_function(
        c,
        receiver,
        &[code, filename, line, column, cached, produce, parsing_context, extensions, params],
    )?;
    let function = get(c, result, "function")?;
    for name in ["cachedDataProduced", "cachedData", "cachedDataRejected"] {
        let value = get(c, result, name)?;
        if !is_undefined(c, value) {
            c.assign_named_property_rooted(function, name, value)?;
        }
    }
    Ok(function)
}

/// `vm.measureMemory(options)`: Quench's heap as one estimate. An eager measurement
/// resolves at once; a default one waits for a collection Quench does not report.
pub(crate) fn measure_memory(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    super::v8_shared_vm::warn_experimental_once(c, "\0warned:vm.measureMemory", MEASURE_MEMORY_WARNING)?;
    let options = argument(c, args, 0);
    let options = options_object(c, options)?;
    let default_mode = c.string_rooted(MEASURE_MODES[0]);
    let mode = option_or(c, options, "mode", default_mode)?;
    let default_execution = c.string_rooted(MEASURE_EXECUTIONS[0]);
    let execution = option_or(c, options, "execution", default_execution)?;
    let mode = validate_one_of(c, mode, "options.mode", &MEASURE_MODES)?;
    let execution = validate_one_of(c, execution, "options.execution", &MEASURE_EXECUTIONS)?;
    let promise = c.pending_promise_rooted()?;
    if execution == EAGER_EXECUTION {
        let used = c.heap_memory_composition().used_bytes() as f64;
        let measurement = |c: &mut Context<'_>| -> Result<RootId, RootedError> {
            let result = c.object_rooted()?;
            let estimate = c.number(used);
            c.assign_named_property_rooted(result, "jsMemoryEstimate", estimate)?;
            let range = c.array_rooted(&[estimate, estimate])?;
            c.assign_named_property_rooted(result, "jsMemoryRange", range)?;
            Ok(result)
        };
        let result = c.object_rooted()?;
        let total = measurement(c)?;
        c.assign_named_property_rooted(result, "total", total)?;
        if mode == DETAILED_MODE {
            let current = measurement(c)?;
            c.assign_named_property_rooted(result, "current", current)?;
            let other = c.array_rooted(&[])?;
            c.assign_named_property_rooted(result, "other", other)?;
        }
        c.resolve_promise_rooted(promise, result)?;
    }
    Ok(promise)
}


/// The `vm` module's exports, in Node's order.
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = c.object_rooted()?;
    let script = script_class(c)?;
    c.assign_named_property_rooted(module, "Script", script)?;
    for (name, operation) in [
        ("createContext", "vmCreateContext"),
        ("createScript", "vmCreateScript"),
        ("runInContext", "vmRunInContext"),
        ("runInNewContext", "vmRunInNewContext"),
        ("runInThisContext", "vmRunInThisContext"),
        ("isContext", "vmIsContext"),
        ("compileFunction", "vmCompileFunction"),
        ("measureMemory", "vmMeasureMemory"),
    ] {
        // Ordinary functions in Node, so `new` applies them too.
        let function = c.host_constructor_named(crate::host::shared_vm::operation(operation), name)?;
        c.assign_named_property_rooted(module, name, function)?;
    }
    let constants = constants(c)?;
    c.assign_named_property_rooted(module, "constants", constants)?;
    Ok(module)
}
