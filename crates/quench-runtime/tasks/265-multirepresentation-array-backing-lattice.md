# 265 — Monotone multi-representation array backing lattice

Status: planned

Replace `ArrayStorage { values: Vec<Value>, non_number_count, ... }` with one canonical
storage coproduct whose payload representation is real:

`Undecided -> PackedI32 -> PackedF64 -> PackedValue`

Packed-to-holey and dense-to-dictionary are separate monotone dimensions. Every write
either fits the current representation or invokes one canonical widening kernel that
copies the backing once, updates the element-kind/version fact, and invalidates dependent
views. Reads select an AOT stencil whose output context matches the backing exactly.

This is not a second array object model. Length, capacity, kind, hole state, version,
and payload belong to one `ArrayStorage` sum. Task 32's guard and Tasks 121/123's raw
views derive from that sum. Task 264 can consume `PackedI32` directly as Word32; numeric
regions can consume `PackedF64` without tagged checks; `PackedValue` retains full JS
semantics. Boxing canonicalizes NaN payloads before constructing a tagged `Value`.

V8 stores actual Smi, raw-double, and tagged backings and widens element kinds
monotonically. JavaScriptCore exposes corresponding initial Int32/double/contiguous
backings and conversion operations. The current `non_number_count == 0` predicate does
not provide those representation or memory-traffic benefits.

Acceptance: transitions cover I32 overflow, fractions, `-0`, NaN/infinity, heap values,
holes, delete, growth, prototype-index fallback, and aliasing; widening preserves exact
observable contents and invalidates every raw view; I32/F64 loops contain direct typed
loads/stores in disassembly; backing bytes and conversion counts are reported; Crypto,
Navier-Stokes, and complete-suite alternating A/B pass.

Primary sources:

- V8 elements kinds: <https://v8.dev/blog/elements-kinds>
- V8 raw-double and tagged backing design: <https://v8.dev/blog/fast-properties>
- JavaScriptCore storage constructors/transitions:
  <https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/runtime/JSObject.h>

