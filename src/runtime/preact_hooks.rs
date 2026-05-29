//! Preact-compatible hooks implementation
//! 
//! Provides useState, useEffect, useContext for island components.
//! Server-side SSR rendering.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    static HOOK_CONTEXT: RefCell<Option<HookContext>> = RefCell::new(None);
}

pub struct HookContext {
    state: HashMap<String, serde_json::Value>,
    effects: Vec<Effect>,
    current_hook: usize,
}

pub struct Effect {
    deps: Vec<serde_json::Value>,
    callback: Box<dyn Fn()>,
}

pub fn setup_component_context() {
    HOOK_CONTEXT.with(|ctx| {
        *ctx.borrow_mut() = Some(HookContext {
            state: HashMap::new(),
            effects: vec![],
            current_hook: 0,
        });
    });
}

pub fn teardown_component_context() {
    HOOK_CONTEXT.with(|ctx| {
        *ctx.borrow_mut() = None;
    });
}

pub fn use_state<T: Clone + Serialize + for<'de> Deserialize<'de> + 'static>(initial: T) -> (T, Rc<dyn Fn(T)>) {
    let id = get_hook_id();
    let key = format!("state_{}", id);
    
    let value = HOOK_CONTEXT.with(|ctx| {
        ctx.borrow()
            .as_ref()
            .and_then(|c| c.state.get(&key).cloned())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(|| initial.clone())
    });
    
    let setter = Rc::new(move |new_value: T| {
        HOOK_CONTEXT.with(|ctx| {
            if let Some(c) = ctx.borrow_mut().as_mut() {
                if let Ok(serialized) = serde_json::to_value(&new_value) {
                    c.state.insert(key.clone(), serialized);
                }
            }
        });
    });
    
    increment_hook();
    (value, setter)
}

pub fn use_effect<F>(callback: F, deps: Vec<serde_json::Value>)
where F: Fn() + 'static {
    HOOK_CONTEXT.with(|ctx| {
        if let Some(c) = ctx.borrow_mut().as_mut() {
            c.effects.push(Effect {
                deps,
                callback: Box::new(callback),
            });
        }
    });
    increment_hook();
}

pub fn use_context<T: Clone + Default>(_: T) -> T {
    increment_hook();
    T::default()
}

pub fn use_ref<T: Default>(initial: T) -> Rc<RefCell<Option<T>>> {
    increment_hook();
    Rc::new(RefCell::new(Some(initial)))
}

pub fn use_memo<T: Clone + Serialize + for<'de> Deserialize<'de>>(compute: impl FnOnce() -> T, _deps: &[serde_json::Value]) -> T {
    let id = get_hook_id();
    let key = format!("memo_{}", id);
    
    let result = HOOK_CONTEXT.with(|ctx| {
        ctx.borrow()
            .as_ref()
            .and_then(|c| c.state.get(&key).cloned())
            .and_then(|v| serde_json::from_value(v).ok())
    });
    
    match result {
        Some(v) => v,
        None => {
            let computed = compute();
            HOOK_CONTEXT.with(|ctx| {
                if let Some(c) = ctx.borrow_mut().as_mut() {
                    if let Ok(serialized) = serde_json::to_value(&computed) {
                        c.state.insert(key, serialized);
                    }
                }
            });
            increment_hook();
            computed
        }
    }
}

pub fn use_callback<T>(callback: impl Fn() -> T + 'static, _deps: &[serde_json::Value]) -> Rc<dyn Fn() -> T> {
    increment_hook();
    Rc::new(callback)
}

fn get_hook_id() -> usize {
    HOOK_CONTEXT.with(|ctx| ctx.borrow().as_ref().map(|c| c.current_hook).unwrap_or(0))
}

fn increment_hook() {
    HOOK_CONTEXT.with(|ctx| {
        if let Some(c) = ctx.borrow_mut().as_mut() {
            c.current_hook += 1;
        }
    });
}

pub fn serialize_state() -> Option<String> {
    HOOK_CONTEXT.with(|ctx| {
        ctx.borrow().as_ref().map(|c| serde_json::to_string(&c.state).unwrap_or_default())
    })
}

pub fn render_component(f: impl FnOnce() -> String) -> String {
    setup_component_context();
    let result = f();
    let state = serialize_state();
    teardown_component_context();
    
    match state {
        Some(s) => format!("{}\"<!--state:{}-->", result, s),
        None => result,
    }
}
