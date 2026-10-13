# Splay allocation profile on the current M4 candidate

This is a memory/allocation profile for the active V8-v7 benchmark, Splay. It is not a timing or qualification run: the binary was built with `profile-memory`, and the profile instrumentation materially changes memory use and execution cost.

## Reproduction

- Host: Apple M4, arm64, macOS 26.5.
- Candidate branch: `task61-splay-array-proto-m4`, evidence revision `16347ed0e0acb51a9010796e2f97abdda90477ec`; runtime change under profile: `5501981f5`.
- Profile binary: `target/candidate-b/production/quench-node`, SHA-256 `9db1e8c1a3355aa8c1be66e4616882fc2c829899810b8d8c17785090fbb4f72a`.
- Input: `../task61-splay-step0-post-merge-2026-10-10/profile-input.js`, SHA-256 `ac9c33765fb2968c78c1f116563913f575451d84826ea8a7bdf79a791bb3f345`; setup, exactly 300 `run()` calls, then teardown.
- Environment: `QUENCH_MEMORY=1 QUENCH_MEMORY_PEAK=1`.
- Command: `/usr/bin/time -l target/candidate-b/production/quench-node <profile-input.js>` with stdout and stderr captured here.

`profile.stdout` contains the completion marker. `profile.stderr` is the raw profile, including the allocation census, object-property lifetime rows, per-collection memory composition, and `/usr/bin/time` output. The reported profile instrumentation alone reaches about 147 MB; the process's 242,795,144-byte peak RSS must not be compared with production RSS.

## Findings

At the post-GC-16 checkpoint, the heap had 781,977 occupied slots: 512,272 ordinary object cells and 256,048 arrays. The record spaces had reserved 27,082,752 bytes for objects and 17,072,128 bytes for arrays; the legacy slot arena reserved 1,556,480 bytes. The array record reservation is the largest remaining direct record pool after objects and is 32 bytes per reserved record in this layout.

The checkpoint's combined object-header property-width histogram is dominated by width 0 (256,103 headers, including the 256,048 arrays) and width 2 (504,096). Width 4 accounts for 8,100 headers. Thus a global increase from two to four inline property values would enlarge the common width-2 records to serve a much smaller width-4 population.

Across the 300 `run()` calls, 168,494 ordinary-object lifetime records reached a peak own-property width above the current two inline values. Two Splay program sites account for 167,969 of them: `(program 0, function 38, pc 10)` has 135,970, and `(program 0, function 31, pc 24)` has 31,999. These are lifetime counts, not a count of current live objects or a measured migration counter. Their function names still need resolution before attributing the sites to a specific source operation.

A 16-byte array record would halve the measured 17,072,128-byte reserved array-record pool at the same high-water count, a projected saving of 8,536,064 bytes before allocator/block effects. This is a design projection from the profile, not a measured RSS result. The next Splay memory experiment should evaluate whether array identity can use a 16-byte header plus the shared-elements reference while keeping the dense element path direct and routing rare named properties through the shared property semantics with an exact fallback.

## Splay status

The last M4 stock candidate retake remains excluded due to external Deno load; its diagnostic median was 9,875 Score and 72,925,184 bytes versus Node at 14,954 Score and Bun at 65,044,480 bytes. Neither bar was won. The official M4 trunk distances remain 1.648x Score and 1.781x max RSS; this profile does not change either distance. Linux still needs its own Splay stock result.
