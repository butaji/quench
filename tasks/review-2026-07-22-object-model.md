# Review 2026-07-22 — object model

Audit of `src/value/**` + the eval/builtins paths that touch property
storage. Verdict: three parallel property subsystems; the one actually
used is the least spec-correct. Feeds R4 / R5 / R9 / R15 in
`tasks/refactor-plan.md`.

## The three parallel stores

1. **Legacy store** (live): `Object.properties: IndexMap<String,Value>` +
   `elements: Vec<Value>` + `holes` + `descriptors` + `getters`/`setters`
   + `symbol_properties` (`value/object/helpers.rs:268-288`).
2. **"TComp" store** (dead): `props: IndexMap<Key, Desc>` +
   `vtable: &'static VTable`. Written 3×, read 0× outside
   `src/value/object/` (grep-verified). Disagrees with the live store on
   attribute defaults (`unwrap_or(false)` vs `unwrap_or(true)`).
3. **Hand-rolled eval path**: `eval/member/object_member.rs` does its own
   prototype walk, non-canonical index parsing (`"01"` accepted), ignores
   holes/setters/descriptors, and mints a fresh Date prototype per access.

## Dead / speculative code (delete first, ~530 LOC)

- `value/object/vtable.rs` (274), `value/object/array.rs` (91),
  `Key`/`Desc`/`VTable`/`Slots`/`ThisMode` in helpers.rs (~80),
  `props`/`slots`/`vtable` fields + init. → R4.
- `Getter`/`Setter`/`GetterBody`/`SetterBody` — zero uses outside
  re-export lists. `ObjData::{String,Func,Proxy,Args}` never constructed.
  → R9.
- `Object::new`/`with_prototype` duplicate 25 lines of field init.
- `property.rs:311-371` one-line delegation wrappers (~40 LOC).
- `kind.rs` Display split into 4 helpers purely to dodge the complexity
  lint (~40 LOC for a 13-arm match).

## Duplication (unify)

- Three same-shaped descriptor types: `PropertyFlags`, `PropertyDescriptor`,
  `Desc`. Six accessor types (four dead, above).
- Key ordering ×3 (`keys.rs`, dead `ordinary_own_property_keys`, eval
  loops). Array-index parsing ×2+ (canonical `as_array_index` vs inline
  `parse::<usize>()` in object_member.rs).
- Prototype-chain walks ×3 (`Object::get`, object_member.rs,
  `to_primitive_function`).
- Function property storage ×3 ad-hoc maps (`ValueFunction.properties`
  HashMap — insertion order lost, `NativeFunction`,
  `ClassValue.static_properties_cell`) instead of `Object`.

## Spec bugs in the live model (test262 failures)

Each needs a failing reproducer `#[test]` before the fix (AGENTS.md).

- **Attribute defaults inverted**: `Object::define_own_property` defaults
  writable/enumerable/configurable `true` (`property.rs:261-263`);
  `Object.defineProperty` spec default is `false`.
- **Strict writes swallowed**: `Object::set` silently no-ops writes to
  non-writable props / non-extensible objects (`property.rs:40-49`);
  strict mode must throw TypeError.
- **No ValidateAndApplyPropertyDescriptor**: non-configurable invariants
  never checked.
- **Symbol identity broken**: `Symbol` has no id field (`val.rs:37-40`);
  `symbol_properties` keyed by desc string — two `Symbol("x")` collide,
  all `Symbol()` share key `""`. AGENTS.md mandates `desc\0id`.
- **Key ordering**: `keys.rs:54` excludes the string `"length"`
  unconditionally (`Object.keys({length:1})` → `[]`); array `own_keys`
  lists hole indices; symbols never appear in `own_keys` at all.
- **Seal/freeze uncomputable**: one `extensible` bool; the elements path
  creates no descriptor entries.
- `get_own_property` reports wrong attributes for elements and drops
  `set` when a getter+setter pair exists (`property.rs:215-248`).
- `to_object("ab")` stuffs the whole string into element `"0"` instead of
  per-index chars (`primitive.rs:262-271`).
- Array element `delete` re-inserts `"length"` at the end of insertion
  order (`object.rs:170-173`).

## Linter-limit offenders (→ R15)

`value/function/value_function.rs` 975, `value/val.rs` 700,
`value/error.rs` 574, `value/generator.rs` 690, `eval/object.rs` 539
(the `assign_to_*` family is yet another parallel set path).
`Object::define_own_property` is ~57 lines (> 40).

## Ranked simplifications

1. Delete TComp layer + dead types — ~530 LOC, low risk, no blockers. (R4/R9)
2. `Symbol` id + desc-keyed `symbol_properties` fix — correctness, unblocks
   symbol suites. (R5)
3. Fix attribute defaults + strict-write throwing — reproducer first. (R5)
4. Unify descriptor types; route eval member access through `Object` —
   kills the parallel-store bug class. (R5)
