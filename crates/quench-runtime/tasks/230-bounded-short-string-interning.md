# 230 — Bounded short-string interning as an O(1) equality fast path

Status: planned

Cross-runtime survey finding, scoped carefully against an existing deliberate design
decision. Lua interns *every* string unconditionally: "Lua interns all strings... value
equality [becomes] trivial [because] if two strings point to the same address in
memory, they are obviously the same string." This gives Lua O(1) string `==` for every
string comparison in the language, not only for identifier/property-key comparisons.
[[11-string-representation]] deliberately does **not** do this for JS strings —
its own text states "no semantic dependence on pointer identity for non-key strings" —
and that restriction is correct as a *canonicalization* rule: unlike Lua, JS programs
routinely build large, one-off computed strings (JSON output, concatenated log lines,
parsed input) that would make unconditional whole-string interning a real memory and
insertion-cost liability, unlike Lua's typical short-string-heavy workloads.

This task is not "undo that decision." It proposes the narrower, non-conflicting version
every production JS engine also actually ships alongside per-key interning: an
**additional, bounded, opportunistic** intern table for *short* strings specifically
(commonly a length cutoff in the low tens of characters, matching V8's/SpiderMonkey's
own internal "flat short string" fast paths already implicitly assumed by
[[190-multi-representation-utf16-javascript-strings]]'s flat-representation design). A
short string is cheap to hash and cheap to store once; interning it opportunistically on
creation (not mandatorily — a string that misses the table's capacity or exceeds the
length cutoff simply isn't interned, and its `===` falls back to content comparison, not
a hard failure) gives `===`/strict-equality on the *common* case — short literals,
short computed substrings, single characters from [[227-o1-indexed-string-char-access]]'s
`charAt` fix — an O(1) pointer-compare fast path, without touching [[11]]'s stated
non-interning guarantee for the general (long, dynamically-built) case.

Concrete steps:
1. State the length cutoff and table-capacity policy as named constants (consistent
   with this project's standing "no unexplained numeric threshold" discipline), sized
   from an actual measurement of the V8v7 corpus's short-string population rather than a
   guess.
2. On short-string construction (literal, `charAt` result, small `substring`/`slice`
   result), probe the intern table; on a hit, return the existing `Rc<String>` (or
   equivalent) instead of allocating a new one — this is a strict subset of what [[11]]'s
   key-interning table already does mechanically, reused for a different key space.
3. `===`/strict-equality's fast path checks pointer identity first (O(1)); a miss falls
   back to full content comparison exactly as it does today — this task can never make
   an equality check *wrong*, only sometimes faster, which bounds its correctness risk
   to "did we intern this pointer" rather than to string semantics themselves.

Acceptance: two structurally-identical short strings constructed from different call
sites/times compare `===` via one pointer compare, verified directly; strings above the
length cutoff or evicted from the bounded table fall back to content comparison with
identical results to today; construction cost for short strings is documented (a probe
plus occasional insert) and shown not to regress construction-heavy suites (crypto,
earley-boyer) in an alternating A/B; a long/one-off computed string (JSON serialization
output, e.g.) is confirmed to never enter the intern table, preserving [[11]]'s existing
memory-liability argument for the general case.

Primary sources:
- Lua's unconditional string interning as the source of trivial `==`:
  <https://www.lua.org/doc/jucs05.pdf> (Ierusalimschy, *The Implementation of Lua 5.0*)
- V8 string representation (short flat strings as the relevant fast-path shape this
  task targets): <https://chromium.googlesource.com/v8/v8.git/+/refs/heads/main/docs/objects/strings.md>
