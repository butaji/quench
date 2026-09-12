# 295 — Replace the system allocator with a size-class-tuned allocator

Status: planned

`Cargo.toml` declares no `#[global_allocator]` override — every `Box`/`Rc`/`Vec`
allocation in this VM goes through the platform's default `malloc` (macOS's
`libsystem_malloc`, already visible by name in [[253]]'s own native profiles:
`reports/task253-comparative/native-top.tsv` shows `_xzm_free`/`xzm_realloc` as
measurable entries in deegen's own richards/deltablue samples). A general-purpose
system allocator is tuned for arbitrary, unpredictable allocation patterns from many
unrelated processes; a language-runtime-specific allocator (jemalloc, mimalloc) is
tuned for exactly this project's actual allocation shape — many small, similarly-sized,
short-lived allocations (`Value`-boxed heap objects, `Vec<Value>` slot arrays, frame
temporaries) — and is a standard, well-established substitution every major dynamic
language runtime makes rather than accepting the platform default.

This is a narrow, low-risk, high-confidence win precisely because it requires zero
changes to this project's own allocation *patterns* — it substitutes the allocator
underneath already-existing `Box`/`Rc`/`Vec` usage, complementing rather than competing
with the eventual bump-allocated nursery from [[162-nonmoving-generational-nursery]]
(which, once built, will remove much of the allocation traffic a tuned general
allocator would otherwise still be serving — this task is valuable now, while [[162]]
remains `planned`, and should be re-measured once [[162]] lands in case its benefit
shrinks or vanishes once most hot allocations bypass the general allocator entirely).

Concrete steps:
1. Add `mimalloc` (or `jemalloc`, whichever measures better on this specific
   allocation shape and target platform) as a `#[global_allocator]`.
2. Measure the `_xzm_free`/`xzm_realloc`/`_platform_memset_pattern16` entries already
   visible in [[253]]'s existing native profiles before and after the swap, on the same
   suites, to get a direct, corpus-specific before/after rather than a generic
   benchmark-suite claim from the allocator's own documentation.
3. Confirm no correctness regression from allocator-specific behavior differences
   (alignment guarantees, `realloc` semantics) across the full test corpus.

Acceptance: a tuned allocator is wired as the global allocator; [[253]]'s own
native-profile allocator-related symbols show a measured reduction in sampled time
(or are confirmed unchanged, honestly reported either way) on at least the suites where
they were already visible; alternating A/B on the full V8v7 suite shows no regression
and documents any measured gain; the task explicitly notes this should be re-measured
after [[162]]'s nursery allocator lands, since much of today's `malloc` traffic is
exactly what a bump-pointer nursery is meant to remove.

Primary sources: mimalloc (Microsoft Research, <https://github.com/microsoft/mimalloc>)
and jemalloc (<https://github.com/jemalloc/jemalloc>) — both standard, widely-adopted
replacements for the system allocator in exactly this kind of workload; no further
citation needed beyond confirming the measured effect on this project's own corpus.
