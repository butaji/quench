# Splay RSS floor input: empty-script Quench process on M4

Host: Mac16,10, Apple M4, macOS 26.5, arm64, 10 logical CPUs, 16 GiB. Eleven `quench-node -e ""` processes were measured with `/usr/bin/time -l` using the pinned production binary whose SHA-256 is recorded in the JSON. The raw time reports and pre/post host load are in `empty-script-quench-node-11.json`.

Median maximum RSS was **19,038,208 B** (18.16 MiB); the range was **18,530,304–19,202,048 B**. This measures the empty JavaScript process only. It excludes V8-v7 fixture parsing, the harness, and benchmark `Setup`, so it is the process-floor input, not the Splay K=0 result.

The Splay `[0..9]` literal arrays share one `Rc<Vec<Value>>` per literal site (`ProgramStore::const_array`, used by `SuperConstArrayObject2`), and the fixture does not mutate them. An ArrayRecord therefore needs only its 16 B header and references; the one shared ten-Value backing allocation and its Rc/Vec metadata are excluded from the per-array count and are negligible here. No int32-elements specialization is needed for Splay.

Corrected GC18 live-record estimate: 504,343 two-property objects × 32 B + 8,000 four-property nodes × 48 B + 256,084 arrays × 16 B = **20,620,320 B**. The observed 2× occupancy relation at 1,562,164 high-water slots implies 781,082 live heap identities across all kinds, not only the 768,427 object/array records. Thus 1.5× capacity requires 1,171,623 locators (rounded up); the roughly 12,655 remaining live identities are legacy cell families whose payloads are excluded below. Bun 1.4.2 with `BUN_JSC_useJIT=0` measured a one-line process maximum RSS of 21,118,976–21,135,360 B over three runs; its 61,898,752 B Splay stock maximum differs from that floor by 40,763,392–40,779,776 B (about **1.98×** the estimated live object/array records; an arithmetic difference, not a same-process K=0 run).

| Projection | Estimated total | Margin below Bun Splay bar |
| -- | --: | --: |
| 1.5× records + 4 B locators for all identities + Quench empty-script floor | 54,655,180 B | 7,243,572 B |
| 1.5× records + 8 B pointers for all identities + Quench empty-script floor | 59,341,672 B | 2,557,080 B |
| 2.0× records + no locators for migrated families + Quench empty-script floor | 60,278,848 B | 1,619,904 B |

These are object/array subtotal sums using the 19,038,208 B Quench empty-script median and Bun's 61,898,752 B Splay bar. They exclude remaining LegacyCell payloads and identity storage, block metadata, allocator slack, harness/setup overhead and untracked resident pages, so they are lower-bound projections, not a win result.

That budget requires `SlotEntry` to be a separate compact identity table containing only a payload offset or pointer. It cannot retain `Option<Cell>` inline: the largest remaining Cell variant would preserve the 56 B slot stride and erase the projected saving. Non-migrated Cell families must also live in separate LegacyCell blocks, with the slot table pointing to them.

This is a paper projection, not a win result. The separate b72 fixed-work Splay K=0 run (fixture/harness loading and Setup, zero `run()` calls) peaked at 65,568,768 B, already 3,670,016 B above Bun's stock RSS bar. That older setup-state measurement conflicts with the simplified additive empty-process-floor model and must be remeasured and attributed on an architecture candidate. Do not add K=0 to the live-record projection because setup objects may overlap the GC18 census.
