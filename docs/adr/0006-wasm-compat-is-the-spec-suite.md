# Wasm compatibility is the spec suite

Quench’s Wasm pass bar is the entire vendored specification testsuite, including in-flight proposal tests (threads, custom-descriptors, wide-arithmetic), not Node’s WebAssembly surface and not Wasmi’s claimed core-suite compliance. A skip list would make “100%” unmeasurable; unfinished proposals stay on the bar and are staged last rather than excluded.

**Considered Options**: core-root only (Wasm 3.0, no `proposals/`); core now with proposals as a named later bar.
