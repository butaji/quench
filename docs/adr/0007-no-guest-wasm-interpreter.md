# No guest Wasm interpreter

The shared runtime is the only Wasm executor. Third-party libraries may parse,
validate, and read wast scripts, but they must not provide fallback execution or
alternate semantics.
