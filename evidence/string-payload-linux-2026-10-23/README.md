# String payload overhead on V8-v7 RegExp: the hypothesis does not hold

Question: is per-allocation overhead on string buffers (the `Rc` header, glibc's chunk
header and 16-byte rounding) large enough on RegExp to remove the 400–560 KiB it needs
under MemoryFirst? Answer: no. The ASCII buffers carry 169 KiB of overhead in all. An ideal
headerless store would remove at most 144 KiB of it, and a one-word thin header 22 KiB.
Nothing was implemented.

## Method

Base: v2-gc `929543e53d`. Build: `iteration` profile, `--features profile-memory`. A
temporary hook, not committed, runs at a chosen collection. It walks every occupied string
record, following each unflattened rope's halves, and collects every heap block behind a
string. It deduplicates the blocks by address and sizes each one with
`malloc_usable_size` + 8 (the chunk header). Under MemoryFirst the allocator is glibc.
It also prints glibc's `mallinfo2`.

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB.

Command:
`QUENCH_MEMORY_POLICY=memory-first QUENCH_STRING_CENSUS_BEFORE=N quench-node regexp.js`

The fixture is `base.js` + `regexp.js` + the runner.

## Census before collection 2000

Most strings own no buffer. Of the 21,037 occupied string records and the rope leaves under
them, 15,240 strings are `Short`, stored inline.

| Block kind | Refs | Distinct blocks | Text bytes | Malloc chunk bytes | Chunk − text |
| -- | --: | --: | --: | --: | --: |
| `Ascii(Rc<str>)` | 5,964 | 5,659 | 514,458 | 687,504 | 173,046 |
| `AsciiSlice` record | 294 | 294 | — | 18,880 | 18,880 |
| `Wide(Rc<[u16]>)` | 262 | 127 | 311,582 | 315,456 | 3,874 |
| `WideSlice` record | 135 | 135 | — | 8,672 | 8,672 |
| `JsStringInner` node | 705 | 705 | — | 45,696 | 45,696 |
| `Aside` (rope halves) | 429 | 429 | — | 21,600 | 21,600 |
| **All string blocks** | | 7,349 | 826,040 | 1,097,808 | 271,768 |

Before collection 500 the totals are similar. There, `Ascii` has 6,782 blocks, 546,841 text
bytes, 752,640 chunk bytes and 205,799 bytes of overhead.

Modelled savings for the same blocks before collection 2000. The model uses glibc's chunk
formula `max(32, (request + 8 + 15) & !15)`. For `Ascii` the formula gives 682,928 bytes;
`malloc_usable_size` measured 687,504.

| Design | `Ascii` chunk bytes | Saved |
| -- | --: | --: |
| Today: `Rc<str>`, 16-byte header | 682,928 | — |
| Thin header: one 8-byte word holding count and length | 660,528 | 22,400 (22 KiB) |
| Headerless and 8-byte aligned (ideal size-classed space) | 535,912 | 147,016 (144 KiB) |

Even the ideal applied to every string block saves 191 KiB: 1,091,552 → 900,120 bytes. That
is under half of the 400 KiB needed.

## What dominates

The board binary is `qb9`, a static production build of v2-gc `d2a77bf4e7`. `smaps` at its
RSS peak shows 6.0–6.2 MiB in all:
- 4.2–4.3 MiB anonymous rw;
- 1.3–1.4 MiB of `quench-node` text;
- about 0.2 MiB of rodata and 0.2 MiB of JIT/rwx.

At collection 2000, `mallinfo2` reports 3.73 MB in use and 0.37 MB free in the main arena.
Live string blocks take 1.10 MB of that in-use total:
- 826 KB is text;
- 192 KB is per-block overhead on ASCII text and slices;
- 80 KB is rope, slice and wide-string structure.

String records reserve another 549 KB (`string_record_reserved_bytes`) for ~18.5K live
records. In the composition report, the other accounted structures are below 0.8 MB
together. The largest are:
- shape table: 229 KB;
- dense array elements: 185 KB;
- slot arena and property arena: 65 KB each.

The rest of the in-use total is outside the heap accounting, about 1.3 MB.

## The census figure was inflated

The earlier census reported 2.19 MB for `string_buffers_bytes`. That figure counts work done
by the report itself. `JsString::memory_parts` calls `flat()`, which flattens every
unflattened rope and keeps the flat copy for the string's life. A `QUENCH_MEMORY` or
`QUENCH_MEMORY_PEAK` report therefore materializes RegExp's input-variant ropes and counts
them: 2,074,227 bytes, against 1,097,808 bytes of real string blocks without a report. It
also counts every `Short` string as a buffer of 0 bytes, so `string_buffer_count` (18,484)
is a count of strings, not of buffers. Fixing the report to use `existing_flat()` would make
it non-perturbing.
