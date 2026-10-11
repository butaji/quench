# Discarded M4 stock Splay attempt for `3d3d1d4e5`

This 11-round stock-harness run is retained for audit, but it is **not a
qualification result** and must not update the scoreboard.

- Host: M4, macOS arm64, `Mac16,10`, 10 logical CPUs.
- Runtime source represented by the measured binary: `3d3d1d4e5`.
- Quench executable SHA-256: `66704b3b7c3e3f0bc29082d0cb5c95cc10841ef90b9e89772f04838443f94f36`.
- Runner SHA-256: `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`.
- Fixture corpus: `64e1860c736c1b899708f4cd646721bc71e53d8b`.
- Raw report SHA-256: `a60ec29b18fcf2bae5c1dd65836fbe7db4779dc1d61720ce3883843dce031b0c`.

The processes completed all 11 rounds with equal output. The host record
nevertheless invalidates the run: unrelated Bun workers were active in round
0, a Deno worker appeared in round 4, and my own recursive binary search was
still running during early samples. The runner could not collect load averages;
the sanitized environment used for this attempt omitted `/usr/sbin`. Therefore
the stock medians below are diagnostic only, not bar distances:

| Engine | Median Score | Median max RSS |
| -- | --: | --: |
| Quench | 8,980 | 116,604,928 B |
| QuickJS | 8,589 | 151,060,480 B |
| Bun, JIT disabled | 9,958 | 64,487,424 B |
| Node `--jitless` | 14,897 | 405,340,160 B |

No scoreboard value is changed from this attempt. Repeat with the full system
`PATH`, including `/usr/sbin`, after the unrelated workers have left the host.
