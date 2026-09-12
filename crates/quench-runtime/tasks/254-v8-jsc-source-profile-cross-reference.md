# 254 — Cross-reference [[253]]'s profiles against V8/JSC source to close specific gaps

Status: in_progress

Companion to [[253-dwarf-comparative-profiling-node-bun]]: once real, symbolized native
profiles of Node and Bun running the V8v7 corpus exist, the top-N hot symbols in each
(builtin names, IC stub names, runtime function names) are concrete pointers into V8's
and JavaScriptCore's actual open-source implementation — read the specific source for
each, the same way this project already did for QuickJS's `libregexp.c`/shape hash table
earlier in this session, rather than reasoning from documentation/blog posts about the
engine's design in the abstract.

This is a materially stronger research method than every prior primary-source round in
this ledger: those rounds (143, 151, 161, 170, 182, 201, 215, 226, 228) cite what V8/JSC
engineers *say* their mechanisms do; this task reads what the *actual, profiled-as-hot*
code *does*, for the exact corpus this project is scored against, closing the gap
between "V8 has an interesting mechanism called X" and "V8 spends measurable time in X
specifically for richards/deltablue/crypto/etc., and here is what X's source actually
does."

Concrete steps:
1. Read pinned official V8 and WebKit source revisions online or from a plain local
   reference checkout. A multi-gigabyte clone is not an acceptance condition when the
   authoritative source file and revision are available directly.
2. For each suite's top-N hot symbols from [[253]]'s profiles, locate and read the
   corresponding V8 (`src/builtins/`, `src/ic/`, `src/objects/`) or JavaScriptCore
   (`Source/JavaScriptCore/runtime/`, `Source/JavaScriptCore/bytecode/`) source
   implementing it.
3. For each hot symbol, produce a short, concrete disposition: (a) this project already
   has an equivalent mechanism, cite the task; (b) this project has a *planned* but
   not-yet-implemented equivalent, cite the task and note this profile is corroborating
   evidence for its priority; (c) this is a genuinely new mechanism not yet represented
   anywhere in this project's 253-task ledger, and needs a new task.
4. Rank the resulting new-task candidates (disposition (c)) by the actual measured
   time-share [[253]]'s profile attributes to them for the suites furthest from [[15]]'s
   target — this gives a corpus-weighted priority order that pure source-reading (this
   project's prior research rounds) could not produce on its own, since it had no
   measured-time-share signal to rank against.

Acceptance: every top-N hot symbol from [[253]]'s Node and Bun profiles (for at least
the suites furthest from [[15]]'s target score) has a stated disposition — (a), (b), or
(c) — backed by actually having read the relevant V8/JSC source, not inferred from its
function name alone; every disposition-(c) finding is written up as a new task the same
way this session's corpus-grounded findings (220-222, 246-248) were, with the specific
source citation and measured time-share as evidence; dispositions (a)/(b) update the
cited existing tasks with this profile as corroborating evidence, following the same
pattern [[161]]'s Richards sample already established for this project's own profiling.

Primary sources: V8 source (`https://chromium.googlesource.com/v8/v8.git`) and WebKit/
JavaScriptCore source (`https://github.com/WebKit/WebKit`), read directly at a named
revision rather than cited secondhand.

## Current disposition after Task 253

The original symbol-by-symbol method is only partially executable. macOS `sample`
symbolizes deegen's Rust code, but both Node and Bun expose much of generated JIT code as
anonymous addresses; Node also exposes idle worker threads. V8's `--prof` resolves JS
functions and tier markers, but Bun has no equivalent artifact in the current tool set.
Do not pretend anonymous JSC addresses are source symbols.

The reliable cross-reference so far is:

- Deegen's 18.48–29.23% generic block executor versus V8's optimized-JS (`*`) bodies:
  existing Tasks 171/158/149/173, with SSA-aware allocation refined by Task 262.
- Deegen frame creation/completion and `Value` drops: existing Tasks 146/172/193/162.
- Crypto's optimized V8 time concentrated in integer bignum loops: new Task 264's
  use-directed Word32 islands and Task 265's real I32 backing, supported by V8's
  `SimplifiedLowering` and element-kind source.
- RegExp matcher plus allocation routines after compilation caching: new Task 263's
  reusable capture-location scratch, supported by the regex crate's allocation-
  amortizing API.
- V8's current copying CFG reducer design: Task 171's immutable rewrite pipeline.

This task remains in progress until JSC generated-code attribution is obtained or its
unavailability is documented as a final tooling limit. Task 262 records the algorithms
that are already supported by reliable evidence; it does not wait on invented Bun
symbolization.
