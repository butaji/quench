//! A Proxy's `[[Set]]` (ECMA-262 10.5.9): what a strict store raises when the `set` trap
//! returns a falsish value.

use super::super::*;

impl<H: Host> Vm<H> {
    /// The `TypeError` of a strict store of the property `key` whose `set` trap returned a
    /// falsish value.
    pub(in crate::vm) fn proxy_set_refused_error(&mut self, p: &ResidualProgram, key: Value) -> JsError {
        let property = self.detail_string(key);
        self.type_error(p, format!("'set' on proxy: trap returned falsish for property '{property}'"))
    }
}
