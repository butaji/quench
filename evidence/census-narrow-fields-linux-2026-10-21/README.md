# Census: how many live fields fit in 32 bits

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Release `quench-node` with
`quench-runtime/profile-memory`, run with `QUENCH_MEMORY=1 QUENCH_MEMORY_PEAK=1`. Census code
is on branch `cand-gc-survival` (7848578e1), not in production. Each workload reports at its
largest live set: the snapshot after the collection with the most live slots, or at exit.
`raw/` keeps that one row per workload. `summary.txt` comes from `narrowsum.py` and
`narrowlen.py`.

A value *fits* a 32-bit slot when it is one of:

- an integer in the 31-bit signed range
- a tagged constant (undefined, null, boolean, hole)
- a heap reference whose index fits in 26 bits, which leaves room for the tag and the
  three family bits

Doubles do not fit, including integral doubles. A *narrow container* is a property vector or
an element backing whose values all fit. Arrays that share a copy-on-write backing are
counted once.

## Findings

- **Slots:** 92.6–100% of live property slots fit. json-parse-inspector has 7.4% doubles and
  raytrace 6.7%; every other workload is at least 99.3%. Element slots are about 100%,
  except splay, whose payload arrays hold doubles.
- **Containers:** 94–99.9% of property slots sit in all-narrow vectors. The exception is
  json-parse at 76%.
- **Object shape:** objects with at most 2 properties dominate. Shares of live objects:
  - splay 97% (755k with 2 properties, plus 383k with none)
  - earley-boyer 99%
  - doxbee 98%
  - acorn 79%
  - frameworks 66–70%
  - hash-map is the exception: 95% of its objects have 3–4 properties.

## What it bounds

An object record is 24 bytes today: an 8-byte header plus 2 inline 8-byte slots. Upper bounds
at the peak live set:

| Layout | splay | hash-map | earley-boyer | acorn | json-parse | doxbee | frameworks |
| -- | --: | --: | --: | --: | --: | --: | --: |
| 16-byte record with 2 narrow inline slots | 8.7 MB | 0.1 | 0.8 | 1.7 | 1.0 | 1.0 | 0.1 |
| all narrow containers at 4 bytes per slot | 6.1 MB | 4.4 | 0.8 | 3.2 | 3.2 | 0.6 | 0.2–0.3 |

Peak RSS for reference: splay 38 MB, hash-map 25, earley-boyer 8.4, acorn 37, json-parse 85,
doxbee 53, frameworks 24–32.

This is the largest RSS lever any census has found. The cost is a decode on every inline
field read and a widening transition when a non-fitting value is stored. Both reach the
interpreter, the specialized tier and the native field paths, so speed must be measured
before any claim. The candidate design goes on Linear as its own issue.
