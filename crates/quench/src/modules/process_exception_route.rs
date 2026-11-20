//! Source-only common reduction of process_shared_vm's exception route.
//! Both Runtime and NativeContext must call this authority after integration;
//! do not leave a second copy of the route in the Runtime path.
use crate::host::{NodeHost, SharedNodeState};
use quench_runtime::{NativeContext, RootId, RootedError, Runtime, Value};
use std::{cell::RefCell, rc::Rc};

pub(crate) trait Context {
    fn shared(&mut self) -> Rc<RefCell<SharedNodeState>>;
    fn text(&mut self, text: &str) -> RootId;
    fn number(&mut self, value: f64) -> RootId;
    fn value(&self, root: RootId) -> Option<Value>;
    fn get(&mut self, object: RootId, key: RootId) -> Result<RootId, RootedError>;
    fn call(
        &mut self,
        function: RootId,
        receiver: RootId,
        args: &[RootId],
    ) -> Result<RootId, RootedError>;
    fn subtract(&mut self, left: RootId, right: RootId) -> Result<RootId, RootedError>;
    fn truthy(&mut self, value: RootId) -> Result<bool, RootedError>;
    /// Runtime owns returned roots; NativeContext owns them in the callback
    /// frame. A NativeContext adapter therefore leaves disposal to its frame.
    fn release_owned(&mut self, root: RootId);
}
impl Context for NativeContext<'_, NodeHost> {
    fn shared(&mut self) -> Rc<RefCell<SharedNodeState>> {
        self.host_mut().shared_state()
    }
    fn text(&mut self, text: &str) -> RootId {
        self.string_rooted(text)
    }
    fn number(&mut self, value: f64) -> RootId {
        self.number(value)
    }
    fn value(&self, root: RootId) -> Option<Value> {
        self.rooted_value(root)
    }
    fn get(&mut self, object: RootId, key: RootId) -> Result<RootId, RootedError> {
        self.get_property_rooted(object, key)
    }
    fn call(
        &mut self,
        function: RootId,
        receiver: RootId,
        args: &[RootId],
    ) -> Result<RootId, RootedError> {
        self.call_rooted(function, receiver, args)
    }
    fn subtract(&mut self, left: RootId, right: RootId) -> Result<RootId, RootedError> {
        self.subtract_rooted(left, right)
    }
    fn truthy(&mut self, value: RootId) -> Result<bool, RootedError> {
        self.truthy_rooted(value)
    }
    fn release_owned(&mut self, _: RootId) {}
}
impl Context for Runtime<NodeHost> {
    fn shared(&mut self) -> Rc<RefCell<SharedNodeState>> {
        self.host_mut().shared_state()
    }
    fn text(&mut self, text: &str) -> RootId {
        self.string_rooted(text)
    }
    fn number(&mut self, value: f64) -> RootId {
        self.root(Value::number(value))
    }
    fn value(&self, root: RootId) -> Option<Value> {
        self.rooted_value(root)
    }
    fn get(&mut self, object: RootId, key: RootId) -> Result<RootId, RootedError> {
        self.get_property_rooted(object, key)
    }
    fn call(
        &mut self,
        function: RootId,
        receiver: RootId,
        args: &[RootId],
    ) -> Result<RootId, RootedError> {
        self.call_rooted(function, receiver, args)
    }
    fn subtract(&mut self, left: RootId, right: RootId) -> Result<RootId, RootedError> {
        self.subtract_rooted(left, right)
    }
    fn truthy(&mut self, value: RootId) -> Result<bool, RootedError> {
        self.truthy_rooted(value).map_err(|error| RootedError {
            error,
            exception: None,
        })
    }
    fn release_owned(&mut self, root: RootId) {
        self.release_root(root);
    }
}
fn property(c: &mut impl Context, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.text(name);
    let value = c.get(object, key);
    c.release_owned(key);
    value
}
pub(crate) fn event(
    c: &mut impl Context,
    name: &str,
    args: &[RootId],
) -> Result<Option<RootId>, RootedError> {
    let process = c.shared().borrow().process_module;
    let Some(process) = process else {
        return Ok(None);
    };
    let name = c.text(name);
    let emit = match property(c, process, "emit") {
        Ok(emit) => emit,
        Err(error) => {
            c.release_owned(name);
            return Err(error);
        }
    };
    let mut values = Vec::with_capacity(args.len() + 1);
    values.push(name);
    values.extend_from_slice(args);
    let result = c.call(emit, process, &values);
    c.release_owned(emit);
    c.release_owned(name);
    result.map(Some)
}
fn auxiliary(c: &mut impl Context, state: RootId, exception: RootId) -> Result<bool, RootedError> {
    let callbacks = property(c, state, "auxiliaryCallbacks")?;
    let length = property(c, callbacks, "length");
    c.release_owned(callbacks);
    let length = length?;
    let one = c.number(1.);
    let index = c.subtract(length, one);
    c.release_owned(length);
    c.release_owned(one);
    let index = index?;
    let number = c.value(index).and_then(Value::as_number);
    c.release_owned(index);
    let mut index =
        number.ok_or_else(|| RootedError::host("auxiliary capture index is not a number"))?;
    while index >= 0. {
        let callbacks = property(c, state, "auxiliaryCallbacks")?;
        let key = c.number(index);
        let callback = c.get(callbacks, key);
        c.release_owned(key);
        let callback = match callback {
            Ok(callback) => callback,
            Err(error) => {
                c.release_owned(callbacks);
                return Err(error);
            }
        };
        let result = c.call(callback, callbacks, &[exception]);
        c.release_owned(callback);
        c.release_owned(callbacks);
        let result = result?;
        let handled = c.value(result).and_then(Value::as_bool) == Some(true);
        c.release_owned(result);
        if handled {
            return Ok(true);
        }
        index -= 1.;
    }
    Ok(false)
}
pub(crate) fn route(
    c: &mut impl Context,
    exception: RootId,
    origin: &str,
) -> Result<bool, RootedError> {
    route_inner(c, exception, origin)
}

fn route_inner(c: &mut impl Context, exception: RootId, origin: &str) -> Result<bool, RootedError> {
    let origin_value = c.text(origin);
    let monitored = event(c, "uncaughtExceptionMonitor", &[exception, origin_value]);
    c.release_owned(origin_value);
    let Some(monitored) = monitored? else {
        return Ok(false);
    };
    c.release_owned(monitored);
    let state = c
        .shared()
        .borrow()
        .process_capture_state
        .ok_or_else(|| RootedError::host("process capture state is not initialized"))?;
    let capture = property(c, state, "captureFn")?;
    let has_capture = c.value(capture).is_some_and(|value| !value.is_null());
    c.release_owned(capture);
    if has_capture {
        // Re-read after the condition, matching the canonical mutable slot.
        let capture = property(c, state, "captureFn")?;
        let result = c.call(capture, state, &[exception]);
        c.release_owned(capture);
        let result = result?;
        c.release_owned(result);
        return Ok(true);
    }
    if auxiliary(c, state, exception)? {
        return Ok(true);
    }
    let origin = c.text(origin);
    let result = event(c, "uncaughtException", &[exception, origin]);
    c.release_owned(origin);
    let Some(result) = result? else {
        return Ok(false);
    };
    let handled = c.truthy(result);
    c.release_owned(result);
    handled
}
