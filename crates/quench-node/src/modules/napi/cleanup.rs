//! Environment cleanup hooks (`napi_add_env_cleanup_hook`, `napi_add_async_cleanup_hook`).
//! Node keeps both kinds in one queue and runs it newest first when the environment ends
//! after a natural exit; an async hook gets its handle and finishes by removing it.

use super::abi::{node_api, CleanupHook};
use super::{api, enter, out, Context, Env, Status, ENVS, NULL_VALUE};
use std::cell::RefCell;
use std::ffi::c_void;

pub(crate) type AsyncCleanupHook = unsafe extern "C" fn(*mut AsyncHandle, *mut c_void);

/// `napi_async_cleanup_hook_handle`: owned by the addon from registration until it
/// removes the hook.
pub(crate) struct AsyncHandle {
    env: *mut Env,
}

#[derive(Clone, Copy)]
enum Hook {
    Sync(CleanupHook, usize),
    Async(AsyncCleanupHook, usize, *mut AsyncHandle),
}

#[derive(Default)]
pub(crate) struct Hooks(RefCell<Vec<Hook>>);

impl Hooks {
    fn push(&self, hook: Hook) {
        self.0.borrow_mut().push(hook);
    }

    fn remove_last(&self, matches: impl Fn(&Hook) -> bool) {
        let mut hooks = self.0.borrow_mut();
        if let Some(index) = hooks.iter().rposition(matches) {
            hooks.remove(index);
        }
    }

    fn pop(&self) -> Option<Hook> {
        self.0.borrow_mut().pop()
    }
}

/// Run every environment's cleanup hooks, newest first, each in an addon entry: Node's
/// FreeEnvironment after a natural exit (process.exit() on the main thread skips them).
pub(crate) fn run_cleanup_hooks(c: &mut Context<'_>) {
    let envs: Vec<*mut Env> = ENVS.with(|envs| envs.borrow().clone());
    for env in envs {
        // SAFETY: environments are leaked at creation and never freed.
        while let Some(hook) = unsafe { &*env }.cleanup_hooks.pop() {
            // A throw inside a cleanup hook is reported by Node only as a fatal error at
            // teardown; the remaining hooks still run.
            let _ = enter(c, env, |_| {
                // SAFETY: addons registered these hooks with their arguments.
                unsafe {
                    match hook {
                        Hook::Sync(run, argument) => run(argument as *mut c_void),
                        Hook::Async(run, argument, handle) => run(handle, argument as *mut c_void),
                    }
                }
                NULL_VALUE
            });
        }
    }
}

node_api! {
    fn napi_add_env_cleanup_hook(env: *mut Env, hook: Option<CleanupHook>, argument: *mut c_void) -> Status {
        api(env, |env, _| {
            let hook = hook.ok_or(Status::InvalidArg)?;
            env.cleanup_hooks.push(Hook::Sync(hook, argument as usize));
            Ok(())
        })
    }

    fn napi_remove_env_cleanup_hook(env: *mut Env, hook: Option<CleanupHook>, argument: *mut c_void) -> Status {
        api(env, |env, _| {
            let hook = hook.ok_or(Status::InvalidArg)?;
            env.cleanup_hooks.remove_last(|candidate| {
                matches!(*candidate, Hook::Sync(run, data) if run as usize == hook as usize && data == argument as usize)
            });
            Ok(())
        })
    }

    fn napi_add_async_cleanup_hook(env: *mut Env, hook: Option<AsyncCleanupHook>, argument: *mut c_void, remove_handle: *mut *mut AsyncHandle) -> Status {
        api(env, |env, _| {
            let hook = hook.ok_or(Status::InvalidArg)?;
            let handle = Box::into_raw(Box::new(AsyncHandle { env: (env as *const Env).cast_mut() }));
            env.cleanup_hooks.push(Hook::Async(hook, argument as usize, handle));
            if remove_handle.is_null() {
                return Ok(());
            }
            out(remove_handle, handle)
        })
    }

    fn napi_remove_async_cleanup_hook(remove_handle: *mut AsyncHandle) -> Status {
        if remove_handle.is_null() {
            return Status::InvalidArg;
        }
        // SAFETY: the handle came from `napi_add_async_cleanup_hook`; removing it ends it.
        let handle = unsafe { Box::from_raw(remove_handle) };
        // SAFETY: environments are leaked at creation and never freed.
        let env = unsafe { &*handle.env };
        env.cleanup_hooks.remove_last(|candidate| matches!(*candidate, Hook::Async(_, _, listed) if listed == remove_handle));
        Status::Ok
    }
}
