# Data-first minimal-runtime architecture

This is the repository's preferred architecture for all new compatibility work.

Describe Node APIs as data first: modules, exports, argument schemas, coercion,
errors, calling conventions, native capabilities, and compatibility evidence.
Generate repetitive Rust registration and JavaScript wrappers from that data.
Keep handwritten JavaScript only for behavior that cannot be expressed by the
schema or a shared adapter. Keep Rust limited to rquickjs integration and
unsafe or OS-bound primitives.

The optimization target is the minimum maintainable LOC, not the minimum raw
source byte count. One generic adapter plus a compact declaration is preferred
to many specialized wrappers. Do not duplicate validation, sync/async
dispatch, callback/promise conversion, export registration, error mapping, or
test scaffolding.

## Required layering

```text
API declarations → normalized IR → generated wrappers/registration/tests
                                      ↓
                              small generic adapters
                                      ↓
                              rquickjs + OS primitives
```

Declarative Rust macros are preferred for Rust-facing schemas and registration.
Procedural macros or a build-time parser may generate artifacts when
`macro_rules!` is insufficient. Do not make a procedural macro parse arbitrary
JavaScript; use a real JavaScript parser in a build tool if source
transformation is genuinely required.

## Migration rule

Every new or substantially changed API must first identify its reusable schema
and adapter. When touching an existing repetitive polyfill, collapse the
repetition into shared data or a generator if doing so reduces total
maintainable LOC without hiding observable Node behavior. Generated files are
outputs; edit their declarations or generator, not the generated artifact.

Readability means readable declarations, IR, and exceptional handwritten
behavior. It does not require preserving duplicated wrapper source.
