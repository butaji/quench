//! `internalBinding('contextify')` (src/node_contextify.cc): compiled scripts, contextified
//! sandboxes and compiled functions.
//!
//! A context is a realm whose global stands for a sandbox object. Node intercepts every
//! global access and forwards it to the sandbox; here the sandbox's own properties are
//! copied onto the realm global before each evaluation and the global's changes copied back
//! after it, while the global's prototype, a proxy, forwards other names to the sandbox live.
//! A context's bookkeeping is a record kept on its sandbox under a private symbol, so it
//! lives exactly as long as the sandbox.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{CompiledScript, DescriptorFields, HostWeakRoot, NativeContext, RootId, RootedError};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

type Context<'a> = NativeContext<'a, NodeHost>;

/// A live `ContextifyScript`: its object, compiled code and source hash.
struct Script {
    identity: HostWeakRoot,
    compiled: CompiledScript,
    source_hash: u64,
}

/// Each live script by the id its private slot holds.
#[derive(Default)]
pub(crate) struct State {
    scripts: HashMap<u64, Script>,
    /// The index the next unnamed context's default name takes.
    pub(crate) next_context_name: u64,
}

static NEXT_SCRIPT: AtomicU64 = AtomicU64::new(1);

/// The private symbols the binding keeps its facts under, built once per VM.
mod slot {
    pub(super) const CONTEXT: &str = "context";
    pub(super) const SCRIPT: &str = "script";
    pub(super) const DECORATED: &str = "decorated";
    pub(super) const ALL: [&str; 3] = [CONTEXT, SCRIPT, DECORATED];
}

/// A context record's fields.
mod record {
    /// The realm global standing for the sandbox.
    pub(super) const GLOBAL: &str = "global";
    /// The sandbox; the global itself for a context made without contextifying.
    pub(super) const SANDBOX: &str = "sandbox";
    /// Keys the realm global owned when it was made, as a null-prototype set.
    pub(super) const INTRINSIC: &str = "intrinsic";
    /// Each key last shared between global and sandbox, with the descriptor it then had.
    pub(super) const FORWARDED: &str = "forwarded";
    /// Evaluations running in the context.
    pub(super) const DEPTH: &str = "depth";
}

/// What precedes a source to start it at a positive line or column offset, which a
/// wrapper origin (a count of lines and columns discounted) cannot express.
const LINE_PADDING: &str = "\n";
const COLUMN_PADDING: &str = " ";
const SOURCE_URL_DIRECTIVE: &str = "sourceURL";
const SOURCE_MAP_URL_DIRECTIVE: &str = "sourceMappingURL";
const FNV64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

fn slots(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:contextify-slots", |c| {
        let slots = c.null_object_rooted()?;
        for name in slot::ALL {
            let symbol = c.private_symbol_rooted(Some(name));
            c.assign_named_property_rooted(slots, name, symbol)?;
        }
        Ok(slots)
    })
}

fn slot_key(c: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    let slots = slots(c)?;
    get(c, slots, name)
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn defined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| !value.is_undefined())
}

fn is_null(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_null())
}

fn integer_argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<i64, RootedError> {
    let value = argument(c, args, index);
    if !defined(c, value) {
        return Ok(0);
    }
    Ok(c.to_number(value)? as i64)
}

fn type_error(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:contextify", |c| {
        let binding = c.object_rooted()?;
        let constructor = c.host_constructor_named(crate::host::shared_vm::operation!("contextifyScript"), "ContextifyScript")?;
        let prototype = get(c, constructor, "prototype")?;
        for (name, operation) in [("createCachedData", "contextifyCreateCachedData"), ("runInContext", "contextifyRunInContext")] {
            let method = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(prototype, key, method, true, true, true)?;
        }
        // V8's prototype template lists `constructor` after the methods.
        let key = c.string_rooted("constructor");
        c.delete_property_rooted(prototype, key)?;
        c.define_data_property_rooted(prototype, key, constructor, true, false, true)?;
        let key = c.string_rooted("ContextifyScript");
        c.define_data_property_rooted(binding, key, constructor, true, true, true)?;
        for (name, operation) in [
            ("compileFunction", "contextifyCompileFunction"),
            ("makeContext", "contextifyMakeContext"),
            ("isContext", "contextifyIsContext"),
        ] {
            let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(binding, key, function, true, true, true)?;
        }
        Ok(binding)
    })
}

/// The binding's `ContextifyScript`, the base of `vm.Script`.
pub(crate) fn script_class(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let binding = binding(c)?;
    get(c, binding, "ContextifyScript")
}

/// The value of the last `//# name=value` directive in `source`, or undefined.
fn magic_comment(c: &mut Context<'_>, source: &str, name: &str) -> RootId {
    let found = source.lines().rev().find_map(|line| {
        let comment = line.trim_start().strip_prefix("//")?;
        let comment = comment.strip_prefix(['#', '@'])?.trim_start();
        let value = comment.strip_prefix(name)?.trim_start().strip_prefix('=')?.trim();
        (!value.is_empty() && !value.contains(char::is_whitespace)).then_some(value)
    });
    match found {
        Some(value) => c.string_rooted(value),
        None => c.undefined(),
    }
}

fn source_hash(source: &str) -> u64 {
    source.bytes().fold(FNV64_OFFSET, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(FNV64_PRIME))
}

/// The cached data for a source: the engine's cache tag, then the source's FNV-1a hash.
fn cached_data(c: &mut Context<'_>, source_hash: u64) -> Result<RootId, RootedError> {
    let bytes = [super::v8_shared_vm::cached_data_tag().to_le_bytes().as_slice(), &source_hash.to_le_bytes()].concat();
    super::buffer_shared_vm::from_vec(c, bytes)
}

/// The cache facts V8 reports for a compilation. Quench compiles from source, so it
/// consumes supplied data as V8 consumes data it accepts: never rejected.
fn report_cache(c: &mut Context<'_>, target: RootId, source: &str, cached: RootId, produce: RootId) -> Result<(), RootedError> {
    if defined(c, cached) {
        let rejected = c.boolean(false);
        c.assign_named_property_rooted(target, "cachedDataRejected", rejected)?;
    } else if c.truthy_rooted(produce)? {
        let data = cached_data(c, source_hash(source))?;
        c.assign_named_property_rooted(target, "cachedData", data)?;
        let yes = c.boolean(true);
        c.assign_named_property_rooted(target, "cachedDataProduced", yes)?;
    }
    Ok(())
}

/// Where a guest source starts: Node's line and column offsets. A negative offset is a
/// wrapper origin (lines and columns positions discount); a positive one, whitespace
/// before the source.
#[derive(Clone, Copy)]
struct Offsets {
    line: i64,
    column: i64,
}

impl Offsets {
    fn read(c: &mut Context<'_>, args: &[RootId], line: usize, column: usize) -> Result<Self, RootedError> {
        Ok(Self { line: integer_argument(c, args, line)?, column: integer_argument(c, args, column)? })
    }

    fn padding(self) -> String {
        let count = |offset: i64| usize::try_from(offset.max(0)).unwrap_or_default();
        LINE_PADDING.repeat(count(self.line)) + &COLUMN_PADDING.repeat(count(self.column))
    }

    fn origin(self) -> (u32, u32) {
        let count = |offset: i64| u32::try_from(-offset.min(0)).unwrap_or(u32::MAX);
        (count(self.line), count(self.column))
    }
}

/// Compile `source` in `global`'s realm as a Script placed at `offsets`. Compile errors
/// carry no source position to decorate their stacks with.
fn compile(c: &mut Context<'_>, global: RootId, source: &str, filename: &str, offsets: Offsets) -> Result<CompiledScript, RootedError> {
    let text = offsets.padding() + source;
    let (line, column) = offsets.origin();
    c.compile_script_in_realm_with_origin(global, &text, filename, line, column)
}

/// `new ContextifyScript(code, filename, lineOffset, columnOffset, cachedData,
/// produceCachedData, parsingContext)`.
pub(crate) fn script(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if c.new_target_rooted().is_none() || !c.is_object_rooted(receiver)? {
        return Err(type_error(c, "Class constructor ContextifyScript cannot be invoked without 'new'"));
    }
    let source = argument(c, args, 0);
    let source = c.to_string(source)?;
    let filename = argument(c, args, 1);
    let filename = c.to_string(filename)?;
    let offsets = Offsets::read(c, args, 2, 3)?;
    let parsing_context = argument(c, args, 6);
    let global = if defined(c, parsing_context) {
        let record = context_record(c, parsing_context)?;
        get(c, record, record::GLOBAL)?
    } else {
        c.global_root()?
    };
    let compiled = compile(c, global, &source, &filename, offsets)?;
    let cached = argument(c, args, 4);
    let produce = argument(c, args, 5);
    report_cache(c, receiver, &source, cached, produce)?;
    let source_url = magic_comment(c, &source, SOURCE_URL_DIRECTIVE);
    c.assign_named_property_rooted(receiver, "sourceURL", source_url)?;
    let source_map_url = magic_comment(c, &source, SOURCE_MAP_URL_DIRECTIVE);
    c.assign_named_property_rooted(receiver, "sourceMapURL", source_map_url)?;
    let id = NEXT_SCRIPT.fetch_add(1, Ordering::Relaxed);
    let key = slot_key(c, slot::SCRIPT)?;
    let id_value = c.number(id as f64);
    c.define_data_property_rooted(receiver, key, id_value, false, false, false)?;
    let identity = c.host_weak_root(receiver)?;
    let script = Script { identity, compiled, source_hash: source_hash(&source) };
    c.host_mut().shared_state().borrow_mut().vm_scripts.scripts.insert(id, script);
    Ok(receiver)
}

/// `(compiled code, source hash)` of the script `receiver`.
fn registered_script(c: &mut Context<'_>, receiver: RootId) -> Result<(CompiledScript, u64), RootedError> {
    let key = slot_key(c, slot::SCRIPT)?;
    let id = if c.is_object_rooted(receiver)? { c.get_property_rooted(receiver, key)? } else { c.undefined() };
    let id = c.rooted_value(id).and_then(|value| value.as_number());
    let shared = c.host_mut().shared_state();
    let script = id.and_then(|id| {
        let state = shared.borrow();
        state.vm_scripts.scripts.get(&(id as u64)).map(|script| (script.compiled.clone(), script.source_hash))
    });
    script.ok_or_else(|| type_error(c, "Illegal invocation"))
}

/// `ContextifyScript.prototype.createCachedData()`.
pub(crate) fn create_cached_data(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let (_, source_hash) = registered_script(c, receiver)?;
    cached_data(c, source_hash)
}

/// `ContextifyScript.prototype.runInContext(contextifiedObject, timeout, displayErrors,
/// breakOnSigint, breakOnFirstLine)`: a null context is the main context. The timeout and
/// SIGINT break are accepted but not enforced: the runtime's only interrupt terminates its
/// owner for good, which a timed-out script must survive.
pub(crate) fn run_in_context(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (script, _) = registered_script(c, receiver)?;
    let sandbox = argument(c, args, 0);
    let display_errors = argument(c, args, 2);
    let display_errors = !defined(c, display_errors) || c.truthy_rooted(display_errors)?;
    let result = if is_null(c, sandbox) {
        let global = c.global_root()?;
        c.evaluate_compiled_script_in_realm_rooted(global, &script)
    } else {
        let record = context_record(c, sandbox)?;
        within(c, record, |c, global| c.evaluate_compiled_script_in_realm_rooted(global, &script))
    };
    result.map_err(|error| if display_errors { decorate(c, error) } else { error })
}

/// Run `body` against the context `record` names, with the sandbox's state on its global
/// before and the global's changes on the sandbox after.
pub(crate) fn within(
    c: &mut Context<'_>,
    record: RootId,
    body: impl FnOnce(&mut Context<'_>, RootId) -> Result<RootId, RootedError>,
) -> Result<RootId, RootedError> {
    let global = get(c, record, record::GLOBAL)?;
    let sandbox = get(c, record, record::SANDBOX)?;
    let contextified = !c.strict_equal_rooted(global, sandbox)?;
    if contextified {
        forward_sandbox(c, record, sandbox, global)?;
    }
    adjust_depth(c, record, 1.0)?;
    let result = body(c, global);
    adjust_depth(c, record, -1.0)?;
    if contextified {
        reflect_globals(c, record, sandbox, global)?;
    }
    result
}

fn adjust_depth(c: &mut Context<'_>, record: RootId, delta: f64) -> Result<(), RootedError> {
    let depth = get(c, record, record::DEPTH)?;
    let depth = c.to_number(depth)? + delta;
    let depth = c.number(depth);
    c.assign_named_property_rooted(record, record::DEPTH, depth)
}

/// Node's `DecorateErrorStack`: prefix an error's stack once with the source line it was
/// thrown at and a caret under its column.
fn decorate(c: &mut Context<'_>, error: RootedError) -> RootedError {
    let Some(exception) = error.exception else {
        return error;
    };
    match decorate_stack(c, exception) {
        Ok(()) => error,
        Err(failure) => failure,
    }
}

fn decorate_stack(c: &mut Context<'_>, exception: RootId) -> Result<(), RootedError> {
    if !c.is_object_rooted(exception)? {
        return Ok(());
    }
    let decorated = slot_key(c, slot::DECORATED)?;
    let already = c.get_property_rooted(exception, decorated)?;
    if c.truthy_rooted(already)? {
        return Ok(());
    }
    let position = c.error_source_position_rooted(exception)?;
    let stack = get(c, exception, "stack")?;
    let (Some(stack), Some(position)) = (c.string_text(stack)?, position) else {
        return Ok(());
    };
    let source_line = position.source_line.unwrap_or_default();
    let underline: String = source_line
        .chars()
        .take(position.column_number.saturating_sub(1) as usize)
        .map(|character| if character == '\t' { '\t' } else { ' ' })
        .collect();
    let resource = position.script_name.unwrap_or_default();
    let arrow = format!("{resource}:{}\n{source_line}\n{underline}^\n", position.line_number);
    let decorated_stack = c.string_rooted(&format!("{arrow}\n{stack}"));
    c.assign_named_property_rooted(exception, "stack", decorated_stack)?;
    let yes = c.boolean(true);
    c.define_data_property_rooted(exception, decorated, yes, true, false, true)?;
    Ok(())
}

/// The record of the context `sandbox` stands for, or ERR_INVALID_ARG_TYPE.
pub(crate) fn context_record(c: &mut Context<'_>, sandbox: RootId) -> Result<RootId, RootedError> {
    match find_record(c, sandbox)? {
        Some(record) => Ok(record),
        None => Err(super::util_shared_vm::argument_type_error_with_expected(c, sandbox, "contextifiedObject", "an vm.Context")?),
    }
}

pub(crate) fn find_record(c: &mut Context<'_>, sandbox: RootId) -> Result<Option<RootId>, RootedError> {
    if !c.is_object_rooted(sandbox)? {
        return Ok(None);
    }
    let key = slot_key(c, slot::CONTEXT)?;
    let record = c.get_property_rooted(sandbox, key)?;
    Ok(defined(c, record).then_some(record))
}

/// `isContext(object)`.
pub(crate) fn is_context(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let object = argument(c, args, 0);
    let found = find_record(c, object)?.is_some();
    Ok(c.boolean(found))
}

/// `makeContext(sandbox, name, origin, allowStrings, allowWasm, ownMicrotaskQueue,
/// importModuleDynamically)`: a new realm standing for `sandbox`, or for itself when
/// `sandbox` is a symbol (`vm.constants.DONT_CONTEXTIFY`). Returns the context object.
pub(crate) fn make_context(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let sandbox = argument(c, args, 0);
    let global = c.create_realm_rooted()?;
    for name in INTRINSIC_ONLY {
        let key = c.string_rooted(name);
        c.delete_property_rooted(global, key)?;
    }
    install_console(c, global)?;
    let allow_strings = argument(c, args, 3);
    if !c.truthy_rooted(allow_strings)? {
        disallow_code_generation(c, global)?;
    }
    let contextified = c.is_object_rooted(sandbox)?;
    let sandbox = if contextified { sandbox } else { global };
    let record = c.null_object_rooted()?;
    c.assign_named_property_rooted(record, record::GLOBAL, global)?;
    c.assign_named_property_rooted(record, record::SANDBOX, sandbox)?;
    let intrinsic = c.null_object_rooted()?;
    let yes = c.boolean(true);
    for key in c.own_keys_rooted(global)? {
        c.define_data_property_rooted(intrinsic, key, yes, true, true, true)?;
    }
    c.assign_named_property_rooted(record, record::INTRINSIC, intrinsic)?;
    let forwarded = c.null_object_rooted()?;
    c.assign_named_property_rooted(record, record::FORWARDED, forwarded)?;
    let depth = c.number(0.0);
    c.assign_named_property_rooted(record, record::DEPTH, depth)?;
    if contextified {
        let scope = sandbox_scope(c, record, global)?;
        c.set_prototype_rooted(global, scope)?;
    }
    let key = slot_key(c, slot::CONTEXT)?;
    c.define_data_property_rooted(sandbox, key, record, false, false, false)?;
    Ok(sandbox)
}

/// Intrinsics a new realm binds on its global that V8 keeps unnamed.
const INTRINSIC_ONLY: [&str; 4] = ["TypedArray", "AsyncFunction", "GeneratorFunction", "AsyncGeneratorFunction"];

/// V8's console methods, in its order. A context's console reports to the inspector, which
/// Quench has none of, so each does nothing.
const CONSOLE_METHODS: [&str; 24] = [
    "debug", "error", "info", "log", "warn", "dir", "dirxml", "table", "trace", "group", "groupCollapsed",
    "groupEnd", "clear", "count", "countReset", "assert", "profile", "profileEnd", "time", "timeLog", "timeEnd",
    "timeStamp", "context", "createTask",
];

fn install_console(c: &mut Context<'_>, global: RootId) -> Result<(), RootedError> {
    let object = get(c, global, "Object")?;
    let console = c.construct_rooted(object, object, &[])?;
    for name in CONSOLE_METHODS {
        let method = c.host_function_named(crate::host::shared_vm::operation!("contextifyConsoleMethod"), name)?;
        c.assign_named_property_rooted(console, name, method)?;
    }
    let symbol = get(c, global, "Symbol")?;
    let tag = get(c, symbol, "toStringTag")?;
    let name = c.string_rooted("console");
    c.define_data_property_rooted(console, tag, name, false, false, true)?;
    let key = c.string_rooted("console");
    c.define_data_property_rooted(global, key, console, true, false, true)?;
    Ok(())
}

pub(crate) fn console_method(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.undefined())
}

/// `codeGeneration: { strings: false }`: the realm's `eval` and `Function` throw its EvalError.
fn disallow_code_generation(c: &mut Context<'_>, global: RootId) -> Result<(), RootedError> {
    let eval_error = get(c, global, "EvalError")?;
    let operation = crate::host::shared_vm::operation!("contextifyRejectCodeGeneration");
    let eval = c.host_function_named_with_data(operation, "eval", eval_error)?;
    let function = c.host_constructor_named_with_data(operation, "Function", eval_error)?;
    for (name, reject) in [("eval", eval), ("Function", function)] {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(global, key, reject, true, false, true)?;
    }
    Ok(())
}

pub(crate) fn reject_code_generation(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    const MESSAGE: &str = "Code generation from strings disallowed for this context";
    let eval_error = c.host_function_data()?;
    let message = c.string_rooted(MESSAGE);
    let error = c.construct_rooted(eval_error, eval_error, &[message])?;
    Err(c.throw(error))
}

/// The global's new prototype: a proxy over an object inheriting the realm's original
/// prototype, reading names the global lacks from the sandbox.
fn sandbox_scope(c: &mut Context<'_>, record: RootId, global: RootId) -> Result<RootId, RootedError> {
    let main = c.global_root()?;
    let object = get(c, main, "Object")?;
    let create = get(c, object, "create")?;
    let prototype = c.prototype_rooted(global)?;
    let target = c.call_rooted(create, object, &[prototype])?;
    let handler = c.null_object_rooted()?;
    for (trap, operation) in [("get", "contextifyScopeGet"), ("set", "contextifyScopeSet"), ("has", "contextifyScopeHas")] {
        let function = c.host_function_named_with_data(crate::host::shared_vm::operation(operation), trap, record)?;
        c.assign_named_property_rooted(handler, trap, function)?;
    }
    let proxy = get(c, main, "Proxy")?;
    c.construct_rooted(proxy, proxy, &[target, handler])
}

struct Scope {
    record: RootId,
    global: RootId,
    sandbox: RootId,
}

fn scope(c: &mut Context<'_>) -> Result<Scope, RootedError> {
    let record = c.host_function_data()?;
    let global = get(c, record, record::GLOBAL)?;
    let sandbox = get(c, record, record::SANDBOX)?;
    Ok(Scope { record, global, sandbox })
}

/// `get(target, key, receiver)`: the sandbox's property, its self-reference read as the
/// global, or the original prototype's.
pub(crate) fn scope_get(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let Scope { record, global, sandbox } = scope(c)?;
    let target = argument(c, args, 0);
    let key = argument(c, args, 1);
    let global_has = defined_descriptor(c, global, key)?;
    if !global_has {
        // Guest code deleted a shared global while running; the stale sandbox copy goes too.
        if let Some(shared) = remembered(c, record, key)? {
            let current = c.own_property_descriptor_rooted(sandbox, key)?;
            if defined(c, current) && same_descriptor(c, current, shared)? {
                c.delete_property_rooted(sandbox, key)?;
                forget(c, record, key)?;
            }
        }
    }
    if c.has_property_rooted(sandbox, key)? {
        let value = c.get_property_rooted(sandbox, key)?;
        return Ok(if c.strict_equal_rooted(value, sandbox)? { global } else { value });
    }
    c.get_property_rooted(target, key)
}

/// `set(target, key, value, receiver)`: a global write. It defines the global's own
/// property; outside an evaluation it also reaches the sandbox at once.
pub(crate) fn scope_set(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let Scope { record, global, sandbox } = scope(c)?;
    let [target, key, value, receiver] = [0, 1, 2, 3].map(|index| argument(c, args, index));
    if !c.strict_equal_rooted(receiver, global)? {
        let stored = c.set_property_rooted(target, key, value, receiver)?;
        return Ok(c.boolean(stored));
    }
    if let Some(fields) = c.own_property_fields_rooted(sandbox, key)? {
        if fields.get.is_some() || fields.set.is_some() {
            let stored = c.set_property_rooted(sandbox, key, value, sandbox)?;
            return Ok(c.boolean(stored));
        }
    }
    if !c.define_data_property_rooted(global, key, value, true, true, true)? {
        return Ok(c.boolean(false));
    }
    let depth = get(c, record, record::DEPTH)?;
    if c.to_number(depth)? == 0.0 {
        c.set_property_rooted(sandbox, key, value, sandbox)?;
        let descriptor = c.own_property_descriptor_rooted(global, key)?;
        remember(c, record, key, descriptor)?;
    }
    Ok(c.boolean(true))
}

/// `has(target, key)`.
pub(crate) fn scope_has(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let Scope { sandbox, .. } = scope(c)?;
    let target = argument(c, args, 0);
    let key = argument(c, args, 1);
    let found = c.has_property_rooted(sandbox, key)? || c.has_property_rooted(target, key)?;
    Ok(c.boolean(found))
}

fn defined_descriptor(c: &mut Context<'_>, object: RootId, key: RootId) -> Result<bool, RootedError> {
    Ok(c.own_property_fields_rooted(object, key)?.is_some())
}

/// Record `descriptor` as the last state `key` shared.
fn remember(c: &mut Context<'_>, record: RootId, key: RootId, descriptor: RootId) -> Result<(), RootedError> {
    let forwarded = get(c, record, record::FORWARDED)?;
    c.define_data_property_rooted(forwarded, key, descriptor, true, true, true)?;
    Ok(())
}

fn forget(c: &mut Context<'_>, record: RootId, key: RootId) -> Result<(), RootedError> {
    let forwarded = get(c, record, record::FORWARDED)?;
    c.delete_property_rooted(forwarded, key)?;
    Ok(())
}

fn remembered(c: &mut Context<'_>, record: RootId, key: RootId) -> Result<Option<RootId>, RootedError> {
    let forwarded = get(c, record, record::FORWARDED)?;
    Ok(c.own_property_fields_rooted(forwarded, key)?.and_then(|fields| fields.value))
}

/// The fields of a descriptor object as `FromPropertyDescriptor` made it.
fn stored_fields(c: &mut Context<'_>, descriptor: RootId) -> Result<DescriptorFields, RootedError> {
    let mut fields = DescriptorFields::default();
    let flag = |c: &mut Context<'_>, name: &str| -> Result<Option<bool>, RootedError> {
        let value = get(c, descriptor, name)?;
        Ok(defined(c, value).then(|| c.rooted_value(value).and_then(|value| value.as_bool()).unwrap_or(false)))
    };
    if c.has_own_property_rooted(descriptor, "value")? {
        fields.value = Some(get(c, descriptor, "value")?);
        fields.writable = flag(c, "writable")?;
    } else {
        fields.get = Some(get(c, descriptor, "get")?);
        fields.set = Some(get(c, descriptor, "set")?);
    }
    fields.enumerable = flag(c, "enumerable")?;
    fields.configurable = flag(c, "configurable")?;
    Ok(fields)
}

/// Whether two descriptor objects describe the same property state.
fn same_descriptor(c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
    let (left, right) = (stored_fields(c, left)?, stored_fields(c, right)?);
    let same_root = |c: &mut Context<'_>, left: Option<RootId>, right: Option<RootId>| match (left, right) {
        (Some(left), Some(right)) => c.same_value_rooted(left, right),
        (None, None) => Ok(true),
        _ => Ok(false),
    };
    Ok(left.writable == right.writable
        && left.enumerable == right.enumerable
        && left.configurable == right.configurable
        && same_root(c, left.value, right.value)?
        && same_root(c, left.get, right.get)?
        && same_root(c, left.set, right.set)?)
}

/// A copy of the data descriptor `descriptor` whose value is `replacement` when it holds
/// `original`: a context's self-reference is the global inside and the sandbox outside.
fn swap_self_reference(c: &mut Context<'_>, descriptor: RootId, original: RootId, replacement: RootId) -> Result<RootId, RootedError> {
    if !c.has_own_property_rooted(descriptor, "value")? {
        return Ok(descriptor);
    }
    let value = get(c, descriptor, "value")?;
    if !c.strict_equal_rooted(value, original)? {
        return Ok(descriptor);
    }
    let copy = c.object_rooted()?;
    for name in ["value", "writable", "enumerable", "configurable"] {
        let field = if name == "value" { replacement } else { get(c, descriptor, name)? };
        c.assign_named_property_rooted(copy, name, field)?;
    }
    Ok(copy)
}

fn intrinsic(c: &mut Context<'_>, record: RootId, key: RootId) -> Result<bool, RootedError> {
    let intrinsic = get(c, record, record::INTRINSIC)?;
    defined_descriptor(c, intrinsic, key)
}

/// Before an evaluation: the sandbox's own properties become the global's, with the same
/// descriptors; shared names the sandbox lost leave the global.
fn forward_sandbox(c: &mut Context<'_>, record: RootId, sandbox: RootId, global: RootId) -> Result<(), RootedError> {
    let forwarded = get(c, record, record::FORWARDED)?;
    for key in c.own_keys_rooted(forwarded)? {
        let Some(shared) = remembered(c, record, key)? else { continue };
        if defined_descriptor(c, sandbox, key)? {
            continue;
        }
        let own = c.own_property_descriptor_rooted(global, key)?;
        if defined(c, own) && same_descriptor(c, own, shared)? {
            c.delete_property_rooted(global, key)?;
            forget(c, record, key)?;
        }
    }
    for key in c.own_keys_rooted(sandbox)? {
        let descriptor = c.own_property_descriptor_rooted(sandbox, key)?;
        let descriptor = swap_self_reference(c, descriptor, sandbox, global)?;
        if let Some(own) = c.own_property_fields_rooted(global, key)? {
            if own.configurable == Some(false) {
                continue;
            }
            if !intrinsic(c, record, key)? && remembered(c, record, key)?.is_none() {
                continue;
            }
        }
        let defined_on_global = c.define_property_descriptor_rooted(global, key, descriptor);
        if defined_on_global.is_ok() {
            remember(c, record, key, descriptor)?;
        }
    }
    Ok(())
}

/// After an evaluation: global properties the script created or changed become the
/// sandbox's; shared names the script deleted leave the sandbox.
fn reflect_globals(c: &mut Context<'_>, record: RootId, sandbox: RootId, global: RootId) -> Result<(), RootedError> {
    for key in c.own_keys_rooted(global)? {
        let shared = remembered(c, record, key)?;
        if shared.is_none() && intrinsic(c, record, key)? {
            continue;
        }
        let descriptor = c.own_property_descriptor_rooted(global, key)?;
        if let Some(shared) = shared {
            if same_descriptor(c, descriptor, shared)? {
                continue;
            }
        }
        let outside = swap_self_reference(c, descriptor, global, sandbox)?;
        let current = c.own_property_descriptor_rooted(sandbox, key)?;
        let assignable = defined(c, current) && {
            let (outside_fields, current_fields) = (stored_fields(c, outside)?, stored_fields(c, current)?);
            outside_fields.value.is_some()
                && current_fields.value.is_some()
                && outside_fields.writable == current_fields.writable
                && outside_fields.enumerable == current_fields.enumerable
                && outside_fields.configurable == current_fields.configurable
        };
        if assignable {
            let value = get(c, outside, "value")?;
            c.set_property_rooted(sandbox, key, value, sandbox)?;
        } else {
            c.define_property_descriptor_rooted(sandbox, key, outside)?;
        }
        remember(c, record, key, descriptor)?;
    }
    let forwarded = get(c, record, record::FORWARDED)?;
    for key in c.own_keys_rooted(forwarded)? {
        if defined_descriptor(c, global, key)? {
            continue;
        }
        if let Some(shared) = remembered(c, record, key)? {
            let current = c.own_property_descriptor_rooted(sandbox, key)?;
            if defined(c, current) && same_descriptor(c, current, shared)? {
                c.delete_property_rooted(sandbox, key)?;
            }
        }
        forget(c, record, key)?;
    }
    Ok(())
}

/// `compileFunction(code, filename, lineOffset, columnOffset, cachedData,
/// produceCachedData, parsingContext, contextExtensions, params)`: the function, compiled
/// with each context extension as a scope around it, and its cache facts.
pub(crate) fn compile_function(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let source = argument(c, args, 0);
    let source = c.to_string(source)?;
    let filename = argument(c, args, 1);
    let filename = c.to_string(filename)?;
    let offsets = Offsets::read(c, args, 2, 3)?;
    let parsing_context = argument(c, args, 6);
    let record = if defined(c, parsing_context) { Some(context_record(c, parsing_context)?) } else { None };
    let extensions = argument(c, args, 7);
    let extensions = if defined(c, extensions) { c.array_like_values_rooted(extensions)? } else { Vec::new() };
    let params = argument(c, args, 8);
    let mut parameters = Vec::new();
    if defined(c, params) {
        for parameter in c.array_like_values_rooted(params)? {
            parameters.push(c.to_string(parameter)?);
        }
    }
    let scopes: String = (0..extensions.len()).map(|index| format!("with (arguments[{index}]) ")).collect();
    let prefix = format!("(function () {{ {scopes}return function ({}) {{\n", parameters.join(", "));
    let suffix = "\n}; })";
    let (wrapper_lines, _) = quench_runtime::wrapper_origin(&prefix).unwrap_or_default();
    let wrapped = format!("{prefix}{}{source}{suffix}", offsets.padding());
    let (line, column) = offsets.origin();
    let evaluate = |c: &mut Context<'_>, global: RootId| {
        let outer = c.evaluate_specialized_script_in_realm_with_origin_rooted(global, &wrapped, &filename, wrapper_lines + line, column)?;
        let undefined = c.undefined();
        c.call_rooted(outer, undefined, &extensions)
    };
    let function = match record {
        Some(record) => within(c, record, evaluate)?,
        None => {
            let global = c.global_root()?;
            evaluate(c, global)?
        }
    };
    let result = c.object_rooted()?;
    c.assign_named_property_rooted(result, "function", function)?;
    let source_map_url = magic_comment(c, &source, SOURCE_MAP_URL_DIRECTIVE);
    c.assign_named_property_rooted(result, "sourceMapURL", source_map_url)?;
    let cached = argument(c, args, 4);
    let produce = argument(c, args, 5);
    report_cache(c, result, &source, cached, produce)?;
    Ok(result)
}

pub(crate) fn prune(runtime: &mut quench_runtime::Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let dead: Vec<u64> = shared
        .borrow()
        .vm_scripts
        .scripts
        .iter()
        .filter(|(_, script)| !runtime.host_weak_root_is_live(script.identity))
        .map(|(id, _)| *id)
        .collect();
    for id in dead {
        let removed = shared.borrow_mut().vm_scripts.scripts.remove(&id);
        if let Some(script) = removed {
            runtime.release_host_weak_root(script.identity);
        }
    }
}

impl State {
    pub(crate) fn write_snapshot(&self, _: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        crate::host::startup_snapshot::require_quiescent(self.scripts.is_empty(), "a vm.Script")
    }
    pub(crate) fn read_snapshot(&mut self, _: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
        Ok(())
    }
}
