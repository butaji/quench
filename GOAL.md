# Goal: stages 51–65

Bring test262 stages 51–65 to 100% passing through the canonical runner:
Infinity, Iterator, JSON, Map, MapIteratorPrototype, Math, NaN, NativeErrors,
Number, Object, Promise, Proxy, Reflect, RegExp, and
RegExpStringIteratorPrototype. Implement shared intrinsic lookup, descriptors,
prototype/realm identity, coercion, proxy traps, iterator and promise
completion behavior without optimizing through observable behavior. Do not
edit test262 or the harness. Re-run owned stages plus earlier regressions;
finish with clean checks and committed verified changes.
