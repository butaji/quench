//! A new Wasm aggregate's field values, read from the register window its operation names.

use super::*;

impl<H: Host> Vm<H> {
    /// The values `read` answers for registers `base..base + count`, or `failed()` when no
    /// room is left for them: they become the new aggregate's storage.
    pub(in crate::vm) fn wasm_window_values(
        &self,
        base: u16,
        count: u16,
        read: impl Fn(&Self, u16) -> Value,
        failed: impl FnOnce() -> JsError,
    ) -> Result<Vec<Value>, JsError> {
        let mut values = Vec::new();
        values.try_reserve_exact(usize::from(count)).map_err(|_| failed())?;
        values.extend((0..count).map(|offset| read(self, base + offset)));
        Ok(values)
    }
}
