# One unwind, distinct catch rules

Traps, tagged Wasm exceptions, and Dynamic throws share one frame walk while
retaining their distinct matching rules. Layer boundaries must define explicit
conversion for uncaught failures.
