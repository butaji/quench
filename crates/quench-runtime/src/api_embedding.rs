use super::Runtime;
use crate::{Host, JsError, RootId, Value};
use std::fmt;

/// An embedding failure and its retained guest exception, if one was thrown.
/// Release `exception` through its originating runtime after handling it.
/// A fresh `execute` invalidates it along with the runtime's other roots.
#[derive(Debug)]
pub struct RootedError {
    pub error: JsError,
    pub exception: Option<RootId>,
}

impl fmt::Display for RootedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for RootedError {}

impl RootedError {
    /// Report a host failure without manufacturing a guest exception identity.
    pub fn host(message: impl Into<String>) -> Self {
        Self {
            error: JsError(message.into().into()),
            exception: None,
        }
    }

    pub(crate) fn retain<H: Host>(vm: &mut crate::vm::Vm<H>, error: JsError) -> Self {
        Self {
            exception: error.thrown_value().map(|value| vm.root(value)),
            error,
        }
    }
}

impl<H: Host> Runtime<H> {
    /// Count actual heap record/arena capacities and unique live payloads without guest effects.
    pub fn heap_memory_composition(&self) -> crate::HeapMemoryComposition {
        self.vm.embedding_heap_memory_composition()
    }

    /// Scope a native embedding frame in captured Error stacks. Native frames
    /// carry no invented JavaScript filename or source position.
    pub fn with_native_stack_frame<T>(
        &mut self,
        name: &str,
        call: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.vm.embedding_push_native_stack_frame(name);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(self)));
        self.vm.embedding_pop_native_stack_frame();
        match result {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// Format a retained exception after refreshing its value through its live
    /// root. Guest hooks may run; exception reporting must hold this root.
    pub fn format_rooted_error(
        &mut self,
        program: &crate::ResidualProgram,
        error: &mut RootedError,
    ) -> String {
        if let Some(root) = error.exception {
            match self.vm.embedding_value(root) {
                Ok(value) => error.error.replace_thrown_value(value),
                Err(_) => return error.error.to_string(),
            }
        }
        self.vm.format_error(program, &error.error)
    }

    /// Retain the current realm's global object. Requires initialized execution.
    pub fn global_root(&mut self) -> Result<RootId, JsError> {
        self.vm.global_root()
    }

    /// Allocate a host string and retain it before subsequent VM work.
    /// Like every root, it is invalidated by a fresh `execute`.
    pub fn string_rooted(&mut self, text: &str) -> RootId {
        self.vm.string_rooted(text)
    }

    /// Read the contents of a rooted string without applying guest coercion.
    pub fn string_text_rooted(&mut self, value: RootId) -> Result<Option<String>, RootedError> {
        self.vm
            .embedding_string_text(value)
            .map_err(|error| self.retain_error(error))
    }

    /// Produce a diagnostic description without running guest code.
    /// This is not ECMAScript string conversion and never calls guest hooks.
    pub fn detail_string_rooted(&mut self, value: RootId) -> Result<String, RootedError> {
        self.vm
            .embedding_detail_string(value)
            .map_err(|error| self.retain_error(error))
    }

    /// Check an own property without invoking accessors or Proxy traps.
    pub fn has_own_property_rooted(
        &mut self,
        object: RootId,
        name: &str,
    ) -> Result<bool, RootedError> {
        self.vm
            .embedding_has_own_property(object, name)
            .map_err(|error| self.retain_error(error))
    }

    /// Check the VM's unforgeable Error brand without guest property access.
    pub fn is_error_rooted(&mut self, value: RootId) -> Result<bool, RootedError> {
        self.vm
            .embedding_is_error(value)
            .map_err(|error| self.retain_error(error))
    }

    /// Create an Error through the same intrinsic owner used by native contexts.
    pub fn error_rooted(&mut self, message: &str) -> Result<RootId, RootedError> {
        let result = self
            .vm
            .create_embedding_exception(crate::heap::Native::Error, message);
        self.retain_completion(result)
    }

    /// Create and retain an ordinary object using the realm's intrinsic prototype.
    pub fn object_rooted(&mut self) -> Result<RootId, RootedError> {
        let result = self.vm.create_embedding_object();
        self.retain_completion(result)
    }

    /// Create an array of `values` with the active realm's intrinsic prototype.
    pub fn array_rooted(&mut self, values: &[RootId]) -> Result<RootId, RootedError> {
        let result = self.vm.create_embedding_array(values);
        self.retain_completion(result)
    }

    /// A Uint8Array that takes ownership of `bytes` as its backing store.
    pub fn uint8_array_from_rooted(&mut self, bytes: Vec<u8>) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_uint8_array_from(bytes);
        self.retain_completion(result)
    }

    /// Create a plain object whose `[[Prototype]]` is null.
    pub fn null_object_rooted(&mut self) -> Result<RootId, RootedError> {
        let result = self.vm.create_embedding_null_object();
        self.retain_completion(result)
    }

    /// Read a property through the shared VM's coercion/getter/Proxy semantics.
    /// All input roots must belong to this runtime and remain live. Validation
    /// precedes guest effects; both success and thrown values return retained.
    pub fn get_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.get_property_rooted(object, key);
        self.retain_completion(result)
    }

    /// `object[name]` for a key given as Rust text; equivalent to `get_property_rooted` with
    /// a string key, without allocating the key.
    pub fn get_named_property_rooted(
        &mut self,
        object: RootId,
        name: &str,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.get_named_property_rooted(object, name);
        self.retain_completion(result)
    }

    /// Write through the shared `Reflect.set` semantics with an explicit receiver.
    /// Refusal returns `false`; a thrown value is retained in `RootedError`.
    /// All four roots are validated before guest coercion, setters or Proxy traps.
    pub fn set_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
        value: RootId,
        receiver: RootId,
    ) -> Result<bool, RootedError> {
        let result = self.vm.set_property_rooted(object, key, value, receiver);
        result.map_err(|error| self.retain_error(error))
    }

    /// `set_property_rooted` with the receiver `object` and a string key the host spells,
    /// without allocating the key.
    pub fn set_named_property_rooted(
        &mut self,
        object: RootId,
        name: &str,
        value: RootId,
    ) -> Result<bool, RootedError> {
        let result = self.vm.set_named_property_rooted(object, name, value);
        result.map_err(|error| self.retain_error(error))
    }

    /// Apply shared subtraction, retaining guest coercion results and exceptions.
    pub fn subtract_rooted(&mut self, left: RootId, right: RootId) -> Result<RootId, RootedError> {
        let result = self.vm.embedding_subtract(left, right);
        self.retain_completion(result)
    }

    /// Invoke an existing callable with a rooted receiver and arguments.
    /// The VM owns program/realm selection and uses its ordinary call path.
    /// Release the result or error's exception root when finished with it.
    pub fn call_rooted(
        &mut self,
        callee: RootId,
        receiver: RootId,
        args: &[RootId],
    ) -> Result<RootId, RootedError> {
        let result = self.vm.call_rooted(callee, receiver, args);
        self.retain_completion(result)
    }

    /// Construct through the shared VM with an explicit `newTarget`.
    /// Every input root is validated before guest effects. Result and thrown
    /// exception roots remain live until released or a fresh execution begins.
    pub fn construct_rooted(
        &mut self,
        callee: RootId,
        new_target: RootId,
        args: &[RootId],
    ) -> Result<RootId, RootedError> {
        let result = self.vm.construct_rooted(callee, new_target, args);
        self.retain_completion(result)
    }

    fn retain_completion(&mut self, result: Result<Value, JsError>) -> Result<RootId, RootedError> {
        match result {
            Ok(value) => Ok(self.root(value)),
            Err(error) => Err(self.retain_error(error)),
        }
    }

    pub(super) fn retain_error(&mut self, error: JsError) -> RootedError {
        RootedError::retain(&mut self.vm, error)
    }
}

#[cfg(test)]
#[path = "api_embedding_tests.rs"]
mod tests;
