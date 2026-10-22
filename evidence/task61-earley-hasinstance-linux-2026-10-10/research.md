# EarleyBoyer `instanceof` optimization research

## Normative semantics

The TC39 ECMAScript specification defines `OrdinaryHasInstance` in §7.3.21:
<https://tc39.es/ecma262/multipage/abstract-operations.html#sec-ordinaryhasinstance>

The algorithm checks callability, delegates bound functions to the target, rejects non-object left operands, gets `constructor.prototype`, requires that prototype to be an object, then walks `[[GetPrototypeOf]]` until null or a match. The candidate only bypasses the native call frame when `@@hasInstance` resolves to Quench's exact `Function.prototype[@@hasInstance]` native; it enters Quench's existing ordinary algorithm with unchanged operands. Getter, custom method, and proxy behavior remains on the generic path. The direct Node v24.19.0 oracle is recorded alongside the candidate measurements.

## Interpreter optimization context

V8's Ignition write-up describes bytecode peephole optimization as a way to reduce register transfers and interpreter overhead; this is a hypothesis source, not evidence of Quench speedup:
<https://v8.dev/blog/ignition>

The candidate is judged only by alternating production EarleyBoyer measurements with the pinned V8-v7 input and exact stdout equality.
