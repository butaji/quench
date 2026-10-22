# Splay stock diagnostic with one contaminated final round (M4)

This Splay-only stock report uses the current runtime at `dd457a54f` (the
report's source revision `6b4b74c93` adds evidence only). All engines completed
11 valid, output-equal rounds. Deno appeared in host snapshots during round 10,
so the report is **not a clean 11-round retake** and does not update the
official Splay row.

## Identity

- Platform: Apple M4 arm64 macOS 26.5, model Mac16,10, 10 logical CPUs.
- Source runtime: `dd457a54f`; report source revision: `6b4b74c93`; clean tree.
- Quench binary SHA-256: `01c22fa3d6c323159dbad38047b8cecb0ccd38edb77773821df17671c8257331`.
- Runner SHA-256: `101aa4b65250b8bdcc41095d10f2c5eeba22d151e25413c299fe9c15da8af964`.
- Pinned V8-v7 corpus: `64e1860c736c1b899708f4cd646721bc71e53d8b`.
- Raw report SHA-256: `aac8f6d16f3a8d7dcbe2bb6bb28ff7a256fd68a615471dfac439e855f254461e`.

## Ten-round uncontended subset

Rounds 0–9 had one-minute load at or below 2.80 across 10 CPUs and no Deno
worker in the snapshots. These ten rounds are a diagnostic subset, not the
required 11-round result.

| Engine | Median Score | Median max RSS |
| -- | --: | --: |
| Quench | 8,258 | 100,843,520 B |
| Node `--jitless` v26.10.0 | 14,689 | 407,388,160 B |
| Bun 1.4.2 no-JIT | 9,860.5 | 66,035,712 B |
| QuickJS 2026-06-04 | 8,557 | 151,060,480 B |

The ten-round diagnostic ratios are 1.779× Score to Node and 1.527× max RSS to
Bun. They are not qualification distances. The unchanged official M4 row is
1.648× Score / 1.781× max RSS on runtime `17077748a`.

## Contaminated round

Deno was absent through rounds 0–9. During round 10 it appeared after Bun's
sample, at 52.7% CPU; it was at 160.1% in snapshots around Node and Quench.
The campaign maximum one-minute load was only 2.94, but the active worker makes
the final matched round unsuitable for the clean-window gate. The report's
full 11-round medians (Quench 8,230 / 100,909,056 B; Node 14,677; Bun 9,828 /
66,043,904 B; QuickJS 8,557) remain audit-only.

## Reproduction

See the exact engine paths and command in the raw JSON report. It was run with
the stock adaptive V8-v7 harness, 11 rounds, and per-sample load/process
snapshots.
