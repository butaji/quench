//! Constructions decided before the general steps of `Vm::construct_value_with_new_target`.
use super::*;

impl Vm {
    /// The start of `construct_value_with_new_target`. Guest code observes the frame stack;
    /// a native's construction reaches it only through the calls, errors and collections
    /// that make deferred activations frames first. A native constructor that is its own new
    /// target, in the running realm, with arguments that are not objects, where the
    /// construction runs no guest code (`constructs_purely`), is then the native's own
    /// construction: the proxy, constructor and realm steps cannot change its outcome.
    /// `None` leaves every other construction to them.
    pub(super) fn construction_shortcut(&mut self, p: &ResidualProgram, callee: Value, new_target: Value,
        args: &[Value], class: u32) -> Option<Result<Value, JsError>> {
        if !self.constructs_beside_activations(callee) {
            self.materialize_deferred_lane();
            return None;
        }
        let Some(record) = self.heap.function(callee) else {
            return None;
        };
        let FunctionKind::Native(native) = record.kind else {
            return None;
        };
        if callee != new_target || self.record_realm(record) != self.realm.globals || !constructs_purely(native)
            || args.iter().any(|argument| self.is_object_like(*argument))
        {
            return None;
        }
        let constructed = self.construct_native_with_new_target(p, native, args, new_target);
        if let Ok(value) = constructed {
            self.capture_constructed_value(value, callee, class, true);
        }
        Some(constructed)
    }
}

/// Constructors that, called as their own new target with arguments that are not objects,
/// run no guest code: they convert primitives and allocate, reading their prototype from
/// the new target (`Native::constructs_from_new_target`).
fn constructs_purely(native: Native) -> bool {
    matches!(native, Native::Date)
}
