# TOD-172: per-site field-cache tables on zero pages — rejected

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Production profile `quench-node`, compile cache
off.

- Base: v2-gc at dc6f23653.
- Candidate: `cand-rss-zero-caches`.

## Change

The four per-site tables move into an anonymous `MAP_NORESERVE` mapping (`ZeroTable`):
- `field_caches`, 12 bytes per site
- `field_add_indices`, `megamorphic_field_indices` and `inherited_field_indices`, 4 bytes
  each

Each table's empty entry is all-zero bits. Field caches store receiver and holder XOR'd
with their sentinels, and the indices are XOR'd with `u32::MAX`. A cold site's page is
never touched, and a full reset drops pages with `MADV_DONTNEED`.

## Census behind it

At exit, npm scenarios filled 150–970 of 20k–34k sites, spread over 7–35 of 59–101 pages.

## Result: rejected

Rule (TOD-172):
- npm ≤ ×0.995.
- No suite's RSS above ×1.005 and no suite's speed below ×0.99.

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| npm top 300 | 1 | — | ×0.9951 |
| V8-v7 | 10 | ×1.001 | ×0.997 |
| Frameworks | 5 | ×0.994 | ×0.999 |
| JetStream (6 tests) | 5 | ×1.005 | ×0.995 |

The saving is about 0.1 MB per process, below the rule and within the A/A noise floor
(npm ×0.9987). It is far below what the census suggested, so most of those table pages were
likely not resident to begin with. Why was not investigated further. The unit test run hit a
full disk; that does not change the decision.
