# No guest Wasm interpreter

Wasmi is removed from the tree. It is a third-party interpreter that currently owns compile, instantiate, invoke, and the wast harness; it is not Quench’s runtime and must not remain as a fallback or dual execute path. Spec-suite tests stay red until the shared VM can run them.

**Considered Options**: keep Wasmi as a result checker only; keep it as a bootstrap executor until each instruction is implemented.
