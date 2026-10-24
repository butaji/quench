//! Native async-hook subscriptions and snapshots across nested hook emission.

use super::event_constructor_shared_vm::{get, named_set};
use crate::host::NodeHost;
use quench_runtime::{BuiltinData, NativeContext, RootId, RootedError};

#[derive(Clone, Copy)]
pub(super) enum HookKind {
    Init,
    Before,
    After,
    Destroy,
    PromiseResolve,
}
impl HookKind {
    pub(super) const ALL: [Self; 5] = [
        Self::Init,
        Self::Before,
        Self::After,
        Self::Destroy,
        Self::PromiseResolve,
    ];
    pub(super) fn index(self) -> usize {
        match self {
            Self::Init => 0,
            Self::Before => 1,
            Self::After => 2,
            Self::Destroy => 3,
            Self::PromiseResolve => 4,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Before => "before",
            Self::After => "after",
            Self::Destroy => "destroy",
            Self::PromiseResolve => "promiseResolve",
        }
    }
}

pub(super) struct Registry {
    pub(super) api: Option<RootId>,
    counts: [u32; HookKind::ALL.len()],
    depth: usize,
    snapshot: Option<(RootId, [u32; HookKind::ALL.len()])>,
}
impl Registry {
    pub(super) fn new() -> Self {
        Self {
            api: None,
            counts: [0; HookKind::ALL.len()],
            depth: 0,
            snapshot: None,
        }
    }
    fn visible_counts(&self) -> [u32; HookKind::ALL.len()] {
        // A hook set is stable for the current emission, but mutations made
        // by a callback take effect for recursively emitted async resources.
        // Node's async_hooks implementation switches to its updated hook
        // array for those nested emissions.
        if self.depth > 0 {
            self.counts
        } else {
            self.snapshot.map_or(self.counts, |(_, counts)| counts)
        }
    }
}

pub(crate) fn initialize(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    if let Some(api) = context
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .registry
        .api
    {
        return Ok(api);
    }
    let api = context.null_object_rooted()?;
    let fatal_error = context.evaluate_script_rooted(
        quench_js_check::checked_js!(
            r#"(function asyncHookFatalError(error) {
            if (typeof error?.stack === 'string') {
              process._rawDebug(error.stack);
            } else {
              process._rawDebug(`Error: ${String(error)}`);
            }
            process.exit(1);
          })"#
        ),
        "node:async_hooks/fatal-error.js",
    )?;
    let fatal_error_key = context.string_rooted("fatalError");
    context.define_data_property_rooted(api, fatal_error_key, fatal_error, true, false, true)?;
    let operations = context
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("async hook assignment API missing"))?;
    let assign = get(context, operations, "assign")?;
    let assign_key = context.string_rooted("assign");
    context.define_data_property_rooted(api, assign_key, assign, true, false, true)?;
    let global = context.global_root()?;
    let primordials = context
        .host_mut()
        .shared_state()
        .borrow()
        .primordials_module;
    let constructor = if let Some(primordials) = primordials {
        get(context, primordials, "SafeSet")?
    } else {
        get(context, global, "Set")?
    };
    let prototype = get(context, constructor, "prototype")?;
    let hooks = context.construct_rooted(constructor, constructor, &[])?;
    named_set(context, api, api, "hooks", hooks)?;
    for name in ["add", "delete", "has"] {
        let method = get(context, prototype, name)?;
        named_set(context, api, api, name, method)?;
    }
    let keys = HookKind::ALL
        .iter()
        .map(|kind| context.symbol_rooted(Some(kind.name())))
        .collect::<Vec<_>>();
    let keys = context.array_rooted(&keys)?;
    named_set(context, api, api, "keys", keys)?;
    let no_promise = context.symbol_rooted(Some("kNoPromiseHook"));
    named_set(context, api, api, "noPromiseKey", no_promise)?;
    let resource = context.object_rooted()?;
    named_set(context, api, api, "defaultResource", resource)?;
    let retained = context.retain(api)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .async_hooks
        .registry
        .api = Some(retained);
    Ok(api)
}

fn key(
    context: &mut NativeContext<'_, NodeHost>,
    api: RootId,
    kind: HookKind,
) -> Result<RootId, RootedError> {
    let keys = get(context, api, "keys")?;
    get(context, keys, &kind.index().to_string())
}

pub(crate) fn initialize_hook(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let hook = args.first().copied().unwrap_or(undefined);
    let api = context.host_function_data()?;
    for (index, kind) in HookKind::ALL.iter().enumerate() {
        let value = args.get(index + 1).copied().unwrap_or(undefined);
        if !context
            .rooted_value(value)
            .is_some_and(|v| v.is_undefined())
            && !context.is_callable_rooted(value)?
        {
            let error =
                context.type_error_rooted(&format!("hook.{} must be a function", kind.name()))?;
            let code = context.string_rooted("ERR_ASYNC_CALLBACK");
            named_set(context, api, error, "code", code)?;
            return Err(context.throw(error));
        }
    }
    let track = args
        .get(HookKind::ALL.len() + 1)
        .copied()
        .unwrap_or(undefined);
    if !context
        .rooted_value(track)
        .is_some_and(|v| v.is_undefined() || v.as_bool().is_some())
    {
        return Err(super::util_shared_vm::argument_type_error(
            context,
            track,
            "trackPromises",
            "boolean",
        )?);
    }
    let no_promise = context
        .rooted_value(track)
        .is_some_and(|v| v.as_bool() == Some(false));
    let promise = args.get(HookKind::ALL.len()).copied().unwrap_or(undefined);
    for (index, kind) in HookKind::ALL.iter().enumerate() {
        let value = args.get(index + 1).copied().unwrap_or(undefined);
        let key = key(context, api, *kind)?;
        super::event_constructor_shared_vm::set(context, api, hook, key, value)?;
    }
    if no_promise && context.is_callable_rooted(promise)? {
        let error=context.type_error_rooted("The argument 'trackPromises' must not be false when promiseResolve is enabled. Received false")?;
        let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
        named_set(context, api, error, "code", code)?;
        return Err(context.throw(error));
    }
    let key = get(context, api, "noPromiseKey")?;
    let value = context.boolean(no_promise);
    super::event_constructor_shared_vm::set(context, api, hook, key, value)?;
    Ok(undefined)
}

pub(crate) fn enable(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    transition(context, args, true)
}
pub(crate) fn disable(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    transition(context, args, false)
}
fn transition(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    enable: bool,
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let hook = args.first().copied().unwrap_or(undefined);
    let api = context.host_function_data()?;
    let hooks = get(context, api, "hooks")?;
    let has = get(context, api, "has")?;
    let present = context.call_rooted(has, hooks, &[hook])?;
    if context.truthy_rooted(present)? == enable {
        return Ok(hook);
    }
    let mut changes = [0; HookKind::ALL.len()];
    for kind in HookKind::ALL {
        let key = key(context, api, kind)?;
        let value = context.get_property_rooted(hook, key)?;
        changes[kind.index()] = u32::from(context.truthy_rooted(value)?);
    }
    let method = get(context, api, if enable { "add" } else { "delete" })?;
    context.call_rooted(method, hooks, &[hook])?;
    let state = context.host_mut().shared_state();
    let mut state = state.borrow_mut();
    for (count, change) in state.async_hooks.registry.counts.iter_mut().zip(changes) {
        *count = if enable {
            count.wrapping_add(change)
        } else {
            count.wrapping_sub(change)
        };
    }
    Ok(hook)
}

pub(super) fn runtime_has_hooks(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    kind: HookKind,
) -> bool {
    runtime
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .registry
        .visible_counts()[kind.index()]
        != 0
}

pub(super) fn emit_runtime(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    kind: HookKind,
    id: u64,
) -> Result<(), RootedError> {
    if !runtime_has_hooks(runtime, kind) {
        return Ok(());
    }
    let function = runtime.host_function(crate::host::shared_vm::operation("asyncHookDispatch"))?;
    let kind = runtime.root(quench_runtime::Value::number(kind.index() as f64));
    let id = runtime.root(quench_runtime::Value::number(id as f64));
    let result = runtime.call_rooted(function, function, &[kind, id]);
    for root in [function, kind, id] {
        runtime.release_root(root);
    }
    result.map(|root| {
        runtime.release_root(root);
    })
}

pub(crate) fn dispatch(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let index = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(|v| v.as_number())
        .ok_or_else(|| RootedError::host("missing async hook kind"))? as usize;
    let kind = HookKind::ALL
        .get(index)
        .copied()
        .ok_or_else(|| RootedError::host("invalid async hook kind"))?;
    emit(context, kind, &args[1..])?;
    Ok(context.undefined())
}

pub(super) fn has_hooks(context: &mut NativeContext<'_, NodeHost>, kind: HookKind) -> bool {
    context
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .registry
        .visible_counts()[kind.index()]
        != 0
}

pub(crate) fn enabled(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (api, snapshot) = {
        let state = context.host_mut().shared_state();
        let state = state.borrow();
        let registry = &state.async_hooks.registry;
        (registry.api, registry.snapshot)
    };
    let count = if let Some((snapshot, _)) = snapshot {
        let length = get(context, snapshot, "length")?;
        context
            .rooted_value(length)
            .and_then(|v| v.as_number())
            .unwrap_or_default() as u32
    } else {
        let api = api.ok_or_else(|| RootedError::host("async hook registry missing"))?;
        let hooks = get(context, api, "hooks")?;
        let Some(BuiltinData::Set(hooks)) = context.builtin_data_rooted(hooks)? else {
            return Err(RootedError::host("async hook subscriptions are not a Set"));
        };
        hooks.len() as u32
    };
    Ok(context.boolean(count != 0))
}

pub(super) fn emit(
    context: &mut NativeContext<'_, NodeHost>,
    kind: HookKind,
    args: &[RootId],
) -> Result<(), RootedError> {
    if !has_hooks(context, kind) {
        return Ok(());
    }
    let state = context.host_mut().shared_state();
    let (api, snapshot, nested) = {
        let state = state.borrow();
        let registry = &state.async_hooks.registry;
        (registry.api, registry.snapshot, registry.depth > 0)
    };
    let api = api.ok_or_else(|| RootedError::host("async hook registry missing"))?;
    let snapshot = if nested {
        // Keep the outer callback list immutable while allowing a hook enabled
        // by that callback to observe a resource created recursively inside
        // it. Each nested emission therefore gets its own short-lived list.
        let hooks = get(context, api, "hooks")?;
        let Some(BuiltinData::Set(hooks)) = context.builtin_data_rooted(hooks)? else {
            return Err(RootedError::host("async hook subscriptions are not a Set"));
        };
        let snapshot = context.array_rooted(&hooks)?;
        context.retain(snapshot)?
    } else if let Some((snapshot, _)) = snapshot {
        snapshot
    } else {
        let hooks = get(context, api, "hooks")?;
        let Some(BuiltinData::Set(hooks)) = context.builtin_data_rooted(hooks)? else {
            return Err(RootedError::host("async hook subscriptions are not a Set"));
        };
        let snapshot = context.array_rooted(&hooks)?;
        let snapshot = context.retain(snapshot)?;
        let mut state = state.borrow_mut();
        let registry = &mut state.async_hooks.registry;
        registry.snapshot = Some((snapshot, registry.counts));
        snapshot
    };
    state.borrow_mut().async_hooks.registry.depth += 1;
    let result = (|| {
        let callback_key = key(context, api, kind)?;
        for hook in super::events_shared_vm::listener_records(context, snapshot)? {
            let callback = context.get_property_rooted(hook, callback_key)?;
            if context.is_callable_rooted(callback)? {
                let callback = context.get_property_rooted(hook, callback_key)?;
                if let Err(error) = context.call_rooted(callback, hook, args) {
                    if let Some(exception) = error.exception {
                        let Some(value) = context.scope_root(exception) else {
                            return Err(error);
                        };
                        let fatal_error = get(context, api, "fatalError")?;
                        let undefined = context.undefined();
                        let result = context.call_rooted(fatal_error, undefined, &[value]);
                        context.release_root(exception);
                        result?;
                        return Ok(());
                    }
                    return Err(error);
                }
            }
        }
        Ok(())
    })();
    let release = {
        let mut state = state.borrow_mut();
        let registry = &mut state.async_hooks.registry;
        registry.depth -= 1;
        if nested {
            Some(snapshot)
        } else if registry.depth == 0 {
            registry.snapshot.take().map(|(root, _)| root)
        } else {
            None
        }
    };
    if let Some(root) = release {
        context.release_root(root);
    }
    result
}
