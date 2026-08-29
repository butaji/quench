# One Native | Fast | Dynamic ladder

Objects and execution are one ladder of three layers, not per-language types. Wasm enters at Native, JavaScript at Dynamic, Typed TypeScript at Fast or Native. A guard or a box is the only way to change layer. There is no JSObject and no WasmObject.

**Considered Options**: Native data is never an object (only Fast/Dynamic are); three implementations that share names.
