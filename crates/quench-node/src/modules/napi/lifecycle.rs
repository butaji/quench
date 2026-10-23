//! Handle scopes, references, wrapping, finalization and per-environment data.

use super::abi::{node_api, CleanupHook, Finalize};
use super::objects::object;
use super::values::value_type;
use super::{api, enter, js_api, out, Context, Env, NapiValue, Status, Thrown, NULL_VALUE};
use quench_runtime::{HostWeakRoot, RootId, RootedError, ValueType};
use std::cell::Cell;
use std::ffi::c_void;

/// A native finalizer and its arguments.
#[derive(Clone, Copy)]
pub(crate) struct Finalizer {
    pub callback: Finalize,
    pub data: *mut c_void,
    pub hint: *mut c_void,
}

/// A finalizer waiting for the collector to clear its target.
pub(crate) struct WeakFinalizer {
    id: u64,
    target: HostWeakRoot,
    finalizer: Finalizer,
}

thread_local! {
    static NEXT_FINALIZER: Cell<u64> = const { Cell::new(0) };
}

/// Run `finalizer` for `target` once the collector clears it; the id cancels it.
pub(super) fn watch(
    env: &Env,
    c: &mut Context<'_>,
    target: RootId,
    finalizer: Finalizer,
) -> Result<u64, Status> {
    let target = c.host_weak_root(target).napi(env, c)?;
    let id = NEXT_FINALIZER.with(|next| next.replace(next.get() + 1));
    env.finalizers.borrow_mut().push(WeakFinalizer {
        id,
        target,
        finalizer,
    });
    Ok(id)
}

fn unwatch(env: &Env, c: &mut Context<'_>, id: u64) {
    let mut finalizers = env.finalizers.borrow_mut();
    if let Some(index) = finalizers.iter().position(|entry| entry.id == id) {
        let entry = finalizers.remove(index);
        c.release_host_weak_root(entry.target);
    }
}

/// Call a finalizer as an addon entry (Node runs them with JavaScript access).
pub(super) fn finalize(
    c: &mut Context<'_>,
    env: *mut Env,
    finalizer: Finalizer,
) -> Result<(), RootedError> {
    let Some(callback) = finalizer.callback else {
        return Ok(());
    };
    enter(c, env, |env| {
        // SAFETY: the addon's finalizer receives the data and hint it registered.
        unsafe { callback(env, finalizer.data, finalizer.hint) };
        NULL_VALUE
    })
    .map(drop)
}

/// Finalizers whose targets the collector has cleared, removed from the watch list.
pub(super) fn collected(env: &Env, c: &mut Context<'_>) -> Vec<Finalizer> {
    let mut finalizers = env.finalizers.borrow_mut();
    let mut ready = Vec::new();
    finalizers.retain(|entry| {
        let live = c.resolve_host_weak_root(entry.target).is_some();
        if !live {
            c.release_host_weak_root(entry.target);
            ready.push(entry.finalizer);
        }
        live
    });
    ready
}

/// A nested handle scope: the handle and root counts it restores when closed.
pub(crate) struct HandleScope {
    handles: usize,
    roots: quench_runtime::RootScopeMark,
    /// For an escapable scope, the enclosing-scope handle reserved for the escapee.
    escape_slot: Option<usize>,
    escaped: Cell<Option<RootId>>,
}

fn open_scope(env: *mut Env, escapable: bool, result: *mut *mut HandleScope) -> Status {
    api(env, |env, c| {
        let escape_slot = escapable.then(|| {
            let placeholder = c.undefined();
            env.handle(placeholder) as usize - 1
        });
        let scope = HandleScope {
            handles: env.handles.borrow().len(),
            roots: c.root_scope_mark(),
            escape_slot,
            escaped: Cell::new(None),
        };
        out(result, Box::into_raw(Box::new(scope)))
    })
}

fn close_scope(env: *mut Env, scope: *mut HandleScope) -> Status {
    api(env, |env, c| {
        if scope.is_null() {
            return Err(Status::InvalidArg);
        }
        // SAFETY: the scope was created by `open_scope` and is closed once.
        let scope = unsafe { Box::from_raw(scope) };
        if env.handles.borrow().len() < scope.handles {
            return Err(Status::HandleScopeMismatch);
        }
        env.handles.borrow_mut().truncate(scope.handles);
        let kept: Vec<RootId> = scope.escaped.get().into_iter().collect();
        c.release_roots_since(scope.roots, &kept);
        Ok(())
    })
}

/// A `napi_ref`: strong while its count is positive, weak at zero for objects.
pub(crate) struct Reference {
    count: Cell<u32>,
    target: Cell<Target>,
}

#[derive(Clone, Copy)]
enum Target {
    Strong(RootId),
    Weak(HostWeakRoot),
    Collected,
}

impl Reference {
    fn value(&self, c: &mut Context<'_>) -> Option<RootId> {
        match self.target.get() {
            Target::Strong(root) => c.scope_root(root),
            Target::Weak(weak) => c.resolve_host_weak_root(weak),
            Target::Collected => None,
        }
    }

    /// Move between strong and weak holding as the count crosses zero.
    fn transition(&self, c: &mut Context<'_>, strong: bool) -> Result<(), RootedError> {
        let next = match (self.target.get(), strong) {
            (Target::Weak(weak), true) => match c.resolve_host_weak_root(weak) {
                Some(value) => {
                    c.release_host_weak_root(weak);
                    Target::Strong(c.retain(value)?)
                }
                None => Target::Collected,
            },
            (Target::Strong(root), false) if weakly_held(c, root) => {
                let weak = c.host_weak_root(root)?;
                c.release_root(root);
                Target::Weak(weak)
            }
            (target, _) => target,
        };
        self.target.set(next);
        Ok(())
    }

    fn release(&self, c: &mut Context<'_>) {
        match self.target.replace(Target::Collected) {
            Target::Strong(root) => drop(c.release_root(root)),
            Target::Weak(weak) => drop(c.release_host_weak_root(weak)),
            Target::Collected => {}
        }
    }
}

/// Objects, functions and externals can be held weakly; other values stay strong.
fn weakly_held(c: &mut Context<'_>, value: RootId) -> bool {
    matches!(
        c.value_type_rooted(value),
        Some(ValueType::Object | ValueType::Function)
    )
}

fn create_reference(
    env: &Env,
    c: &mut Context<'_>,
    value: RootId,
    count: u32,
) -> Result<*mut Reference, Status> {
    // Before Node-API 10 only objects, functions and symbols could be referenced.
    let kind = value_type(c, value)?;
    if !env.module_version_at_least(10)
        && !matches!(
            kind,
            ValueType::Object | ValueType::Function | ValueType::Symbol
        )
    {
        return Err(Status::InvalidArg);
    }
    let reference = Reference {
        count: Cell::new(count),
        target: Cell::new(Target::Strong(c.retain(value).napi(env, c)?)),
    };
    if count == 0 {
        reference.transition(c, false).napi(env, c)?;
    }
    Ok(Box::into_raw(Box::new(reference)))
}

fn reference<'a>(reference: *mut Reference) -> Result<&'a Reference, Status> {
    // SAFETY: a non-null reference was created by `create_reference` and not deleted.
    unsafe { reference.as_ref() }.ok_or(Status::InvalidArg)
}

/// The wrap record stored for a wrapped object.
struct Wrap {
    native: *mut c_void,
    finalizer: Option<u64>,
}

fn map_get(
    env: &Env,
    c: &mut Context<'_>,
    map: RootId,
    key: RootId,
) -> Result<Option<usize>, Status> {
    let value = c.weak_map_get_rooted(map, key).napi(env, c)?;
    Ok(c.external_address_rooted(value))
}

fn map_set(
    env: &Env,
    c: &mut Context<'_>,
    map: RootId,
    key: RootId,
    address: usize,
) -> Result<(), Status> {
    let value = c.external_rooted(address).napi(env, c)?;
    c.call_rooted(env.weak_map_set, map, &[key, value])
        .napi(env, c)
        .map(drop)
}

fn map_delete(env: &Env, c: &mut Context<'_>, map: RootId, key: RootId) -> Result<(), Status> {
    c.call_rooted(env.weak_map_delete, map, &[key])
        .napi(env, c)
        .map(drop)
}

fn take_wrap(
    env: &Env,
    c: &mut Context<'_>,
    target: NapiValue,
) -> Result<(RootId, Box<Wrap>), Status> {
    let target = object(c, env.value(target)?)?;
    let address = map_get(env, c, env.wraps, target)?.ok_or(Status::InvalidArg)?;
    map_delete(env, c, env.wraps, target)?;
    // SAFETY: wrap records are boxed by `napi_wrap` and removed from the map once.
    Ok((target, unsafe { Box::from_raw(address as *mut Wrap) }))
}

node_api! {
    fn napi_open_handle_scope(env: *mut Env, result: *mut *mut HandleScope) -> Status {
        open_scope(env, false, result)
    }

    fn napi_close_handle_scope(env: *mut Env, scope: *mut HandleScope) -> Status {
        close_scope(env, scope)
    }

    fn napi_open_escapable_handle_scope(env: *mut Env, result: *mut *mut HandleScope) -> Status {
        open_scope(env, true, result)
    }

    fn napi_close_escapable_handle_scope(env: *mut Env, scope: *mut HandleScope) -> Status {
        close_scope(env, scope)
    }

    fn napi_escape_handle(env: *mut Env, scope: *mut HandleScope, escapee: NapiValue, result: *mut NapiValue) -> Status {
        api(env, |env, _| {
            // SAFETY: a live escapable scope from `open_scope`.
            let scope = unsafe { scope.as_ref() }.ok_or(Status::InvalidArg)?;
            let slot = scope.escape_slot.ok_or(Status::InvalidArg)?;
            if scope.escaped.get().is_some() {
                return Err(Status::EscapeCalledTwice);
            }
            let value = env.value(escapee)?;
            scope.escaped.set(Some(value));
            env.handles.borrow_mut()[slot] = value;
            out(result, (slot + 1) as NapiValue)
        })
    }

    fn napi_create_reference(env: *mut Env, value: NapiValue, count: u32, result: *mut *mut Reference) -> Status {
        api(env, |env, c| {
            let value = env.value(value)?;
            let reference = create_reference(env, c, value, count)?;
            out(result, reference)
        })
    }

    fn napi_delete_reference(env: *mut Env, reference_ptr: *mut Reference) -> Status {
        api(env, |_, c| {
            reference(reference_ptr)?.release(c);
            // SAFETY: deleting the reference ends its lifetime.
            drop(unsafe { Box::from_raw(reference_ptr) });
            Ok(())
        })
    }

    fn napi_reference_ref(env: *mut Env, reference_ptr: *mut Reference, result: *mut u32) -> Status {
        api(env, |env, c| {
            let reference = reference(reference_ptr)?;
            let count = reference.count.get() + 1;
            reference.count.set(count);
            if count == 1 {
                reference.transition(c, true).napi(env, c)?;
            }
            if !result.is_null() { out(result, count)?; }
            Ok(())
        })
    }

    fn napi_reference_unref(env: *mut Env, reference_ptr: *mut Reference, result: *mut u32) -> Status {
        api(env, |env, c| {
            let reference = reference(reference_ptr)?;
            let count = reference.count.get().checked_sub(1).ok_or(Status::GenericFailure)?;
            reference.count.set(count);
            if count == 0 {
                reference.transition(c, false).napi(env, c)?;
            }
            if !result.is_null() { out(result, count)?; }
            Ok(())
        })
    }

    fn napi_get_reference_value(env: *mut Env, reference_ptr: *mut Reference, result: *mut NapiValue) -> Status {
        api(env, |env, c| {
            let value = reference(reference_ptr)?.value(c);
            out(result, value.map_or(NULL_VALUE, |value| env.handle(value)))
        })
    }

    fn napi_wrap(env: *mut Env, target: NapiValue, native: *mut c_void, finalize: Finalize, hint: *mut c_void, result: *mut *mut Reference) -> Status {
        js_api(env, |env, c| {
            let target = object(c, env.value(target)?)?;
            if map_get(env, c, env.wraps, target)?.is_some() {
                return Err(Status::InvalidArg);
            }
            let finalizer = match finalize {
                Some(_) => Some(watch(env, c, target, Finalizer { callback: finalize, data: native, hint })?),
                None => None,
            };
            let wrap = Box::into_raw(Box::new(Wrap { native, finalizer }));
            map_set(env, c, env.wraps, target, wrap as usize)?;
            if !result.is_null() {
                out(result, create_reference(env, c, target, 0)?)?;
            }
            Ok(())
        })
    }

    fn napi_unwrap(env: *mut Env, target: NapiValue, result: *mut *mut c_void) -> Status {
        js_api(env, |env, c| {
            let target = object(c, env.value(target)?)?;
            let address = map_get(env, c, env.wraps, target)?.ok_or(Status::InvalidArg)?;
            // SAFETY: wrap records live until removed from the map.
            out(result, unsafe { &*(address as *const Wrap) }.native)
        })
    }

    fn napi_remove_wrap(env: *mut Env, target: NapiValue, result: *mut *mut c_void) -> Status {
        js_api(env, |env, c| {
            let (_, wrap) = take_wrap(env, c, target)?;
            if let Some(id) = wrap.finalizer {
                unwatch(env, c, id);
            }
            if !result.is_null() { out(result, wrap.native)?; }
            Ok(())
        })
    }

    fn napi_type_tag_object(env: *mut Env, target: NapiValue, tag: *const [u64; 2]) -> Status {
        js_api(env, |env, c| {
            let target = object(c, env.value(target)?)?;
            // SAFETY: the addon passes a readable `napi_type_tag`.
            let tag = unsafe { tag.as_ref() }.copied().ok_or(Status::InvalidArg)?;
            if map_get(env, c, env.type_tags, target)?.is_some() {
                return Err(Status::InvalidArg);
            }
            map_set(env, c, env.type_tags, target, Box::into_raw(Box::new(tag)) as usize)
        })
    }

    fn napi_check_object_type_tag(env: *mut Env, target: NapiValue, tag: *const [u64; 2], result: *mut bool) -> Status {
        js_api(env, |env, c| {
            let target = object(c, env.value(target)?)?;
            // SAFETY: the addon passes a readable `napi_type_tag`.
            let tag = unsafe { tag.as_ref() }.copied().ok_or(Status::InvalidArg)?;
            let stored = map_get(env, c, env.type_tags, target)?;
            // SAFETY: stored tags are boxed by `napi_type_tag_object` and never freed.
            out(result, stored.is_some_and(|address| unsafe { *(address as *const [u64; 2]) } == tag))
        })
    }

    fn napi_add_finalizer(env: *mut Env, target: NapiValue, data: *mut c_void, finalize: Finalize, hint: *mut c_void, result: *mut *mut Reference) -> Status {
        api(env, |env, c| {
            finalize.ok_or(Status::InvalidArg)?;
            let target = object(c, env.value(target)?)?;
            watch(env, c, target, Finalizer { callback: finalize, data, hint })?;
            if !result.is_null() {
                out(result, create_reference(env, c, target, 0)?)?;
            }
            Ok(())
        })
    }

    fn node_api_post_finalizer(env: *mut Env, finalize: Finalize, data: *mut c_void, hint: *mut c_void) -> Status {
        api(env, |env, _| {
            finalize.ok_or(Status::InvalidArg)?;
            super::work::push(&env.events, super::work::Event::Finalize(Finalizer { callback: finalize, data, hint }));
            Ok(())
        })
    }

    fn napi_set_instance_data(env: *mut Env, data: *mut c_void, finalize: Finalize, hint: *mut c_void) -> Status {
        api(env, |env, _| {
            env.instance_data.set(Some(Finalizer { callback: finalize, data, hint }));
            Ok(())
        })
    }

    fn napi_get_instance_data(env: *mut Env, result: *mut *mut c_void) -> Status {
        api(env, |env, _| out(result, env.instance_data.get().map_or(std::ptr::null_mut(), |data| data.data)))
    }

    fn napi_add_env_cleanup_hook(env: *mut Env, hook: Option<CleanupHook>, argument: *mut c_void) -> Status {
        api(env, |env, _| {
            let hook = hook.ok_or(Status::InvalidArg)?;
            env.cleanup_hooks.borrow_mut().push((hook, argument as usize));
            Ok(())
        })
    }

    fn napi_remove_env_cleanup_hook(env: *mut Env, hook: Option<CleanupHook>, argument: *mut c_void) -> Status {
        api(env, |env, _| {
            let hook = hook.ok_or(Status::InvalidArg)?;
            let mut hooks = env.cleanup_hooks.borrow_mut();
            if let Some(index) = hooks.iter().rposition(|&(candidate, data)| candidate as usize == hook as usize && data == argument as usize) {
                hooks.remove(index);
            }
            Ok(())
        })
    }

    fn napi_adjust_external_memory(env: *mut Env, change: i64, result: *mut i64) -> Status {
        api(env, |env, _| {
            let total = env.external_memory.get().saturating_add(change).max(0);
            env.external_memory.set(total);
            out(result, total)
        })
    }
}
