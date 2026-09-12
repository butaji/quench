# 41 — Flattened closure capture instead of Environment-chain walks

Status: planned

[[13-environment-frames]] caches lexical resolution via `NameIc`, but a cache miss and a closure's first resolution still walk the `Rc<RefCell<Environment>>` parent chain. For closure-heavy code (splay's `traverse_` callback, richards' task closures, both already identified in [[35-closure-callback-call-caching]]), compile a closure's captured-variable set into a flat, fixed-layout struct at closure-creation time — a De Bruijn-index-to-struct-offset transform performed once when the closure is created — instead of an `Environment` linked list walked at each access.

This turns "walk N parent scopes" into "one fixed-offset load," independent of `NameIc` warm/cold state, and composes with the escape-analysis work in [[24-escape-analysis-scalar-replacement]]: a closure whose captures are all provably non-reassigned can even scalar-replace the capture struct itself.

Acceptance: closure creation for a function with statically known captures produces a fixed-layout struct, verified by inspecting the generated capture representation; access to a captured variable inside the closure body compiles to a fixed-offset load with no chain walk, even on the very first access (not only after a cache warms); a closure capturing a variable later reassigned in an enclosing scope still observes the reassignment correctly.
