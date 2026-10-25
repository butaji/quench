# Compiled RegExp patterns: bitmap sets, lazy wide finders, boxed byte backend

## Finding

The design doc (§8.3a) lists about 0.5 MB of compiled regexps as RegExp's remaining gap. The
memory is held live by the matcher cache and by live RegExp objects.

`rxmem-harness.rs` (a temporary example on `quench-regexp`, not committed) counts live heap
bytes around each pattern. It compiles each of V8-v7 RegExp's 237 distinct patterns
(`extract-patterns.js`), then searches once on an ASCII subject through
`Regex::find_in_subject`, the runtime's path. The base holds 523 KiB live:
- **Each `MatchSet`** held a `[bool; 128]` ASCII table: 123 KiB of the 312 KiB built at
  construction. `MatchSet`s also sit inline in `Program`'s `starts` and `anchored_first`.
- **The UTF-16 required-literal finder** was built eagerly for 110 patterns, at several hundred
  bytes each, though the fixture searches only ASCII subjects.
- **Each `Regex`** was 568 bytes. Five of its fields serve only the opt-in `regex`-crate
  backend (`QUENCH_COMPILED_REGEXP`).

## Change

- `AsciiTable` is two `u64` words. A test loads a word and shifts it, and the unit is masked
  to ASCII inside `contains`, so callers no longer repeat the mask.
- `CompleteMatcher::required_wide` is a `OnceCell`, built by the first wide search.
- The byte backend's fields are one `Option<Box<ByteBackend>>`.

Result: 523 -> 309 KiB live (-41%), with `Regex` 568 -> 464 bytes.

## Measurement

Static production builds, unbolted, MemoryFirst, binaries evicted:
- **Base:** v2-gc `8fb03c6810`.
- **Change:** `cand-gc-regexp-bitmap`.
- **Runs:** 15 interleaved per row (`binab.py`, `v8-15-interleaved.txt`). Medians and worst
  runs are in KiB.
- **Host:** 4-vCPU Intel Xeon @ 2.80GHz x86_64 VM, 16 GB.

| Row | Base median | Change median | Base worst | Change worst | Score |
| -- | --: | --: | --: | --: | --: |
| RegExp | 5984 | 5728 | 6056 | 5856 | 2090 -> 2066 |
| RegExp `--jit` | 6008 | 5880 | 6168 | 5956 | 2198 -> 2266 |
| Crypto (control) | 3152 | 3176 | 3272 | 3304 | 2447 -> 2435 |
| Crypto `--jit` (control) | 3200 | 3196 | 3360 | 3272 | 2275 -> 2261 |
| EarleyBoyer | 6392 | 6388 | 6600 | 6516 | 3683 -> 3559 |
| EarleyBoyer `--jit` | 6532 | 6552 | 6860 | 6844 | 3572 -> 3477 |

Scores are within run-to-run noise.

## Correctness

All of these pass:
- `quench-regexp` tests, with and without `QUENCH_COMPILED_REGEXP`;
- `quench-runtime` lib regexp tests;
- test262 `built-ins/RegExp`, `language/literals/regexp`, and `String.prototype` `match`,
  `matchAll`, `replace`, `replaceAll`, `search` and `split`: 2456 of 2456.
