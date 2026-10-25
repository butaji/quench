# npm top-300 board p58 (v2-npm 0b9f977489)

This board used the same host, oracles, rounds and environment as
`npm-top300-board-p57-linux-2026-10-25`. Since p57 it adds one change: a
compile-cache entry's pages are returned to the system once the program is decoded
(`Host::kept_bytes_read`).

| mode | passing in every run | fastest wall | lowest peak RSS |
|---|---|---|---|
| no JIT | 299 / 300 | 285 (p57: 283) | 290 (p57: 291) |
| JIT | 299 / 300 | 279 (p57: 281) | 293 (p57: 289) |

The 300th scenario is the musl rollup addon, which Node fails too.

## Changes from p57

- undici's peak RSS is now below Bun's.
- typescript-eslint without JIT is within 2% of Bun's wall time (676 vs 663 ms).
- Quench still uses less CPU than Bun on that row.

`losing-rows.txt` lists the remaining rows by cause, with the same categories
as p57. Wall-time ranks of rows under 100 ms change between boards on this
host. Bun's own timing for ws, baseline-browser-mapping and nanoid changed by
20–40% between p57 and p58.
