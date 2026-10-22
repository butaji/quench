# M4 Splay direct-object record screen

This is the Splay-only fixed-work screen for the experimental direct ordinary-object record candidate. It is not an all-eight decision or a stock-harness qualification.

## Provenance

- Host: arm64 macOS 25.5, Mac16,10, 10 logical CPUs; `rustc 1.99.0` / LLVM 23.1.2.
- Trunk source: `4a3925c7b5ccf0d5c16560ddacdf305ca9ae71ac` (runtime code includes the gated `3d3d1d4e5` speed head).
- Candidate source: `f1fbd093d8089563afebb8a6eeb38b7abb9936dd`, built from that trunk.
- Baseline executable SHA-256: `66704b3b7c3e3f0bc29082d0cb5c95cc10841ef90b9e89772f04838443f94f36`.
- Candidate executable SHA-256: `1e487613191b2bf6e11eadd90e3658cc94ecabe7a89d592fc641ba425e55c085`.
- Runner SHA-256: `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`.
- Work: Splay, 300 `run()` calls; each round also ran the matched setup-only K=0 input. The schema-5 report records 11/11 valid, output-equal, contention-clean paired rounds.
- Raw report: [`splay-fixed-work-trunk-3d-vs-record-11.json`](splay-fixed-work-trunk-3d-vs-record-11.json).

## Paired results

The speed delta is computed per round after subtracting that engine's K=0 run, then compared as a paired percentage. The interval is the task61 guard's 20,000-replicate paired percentile bootstrap (95%; seed `0x61_F1_7E_D5_2026_1010`).

| Metric | Trunk | Candidate | Candidate delta [95% interval] |
|---|---:|---:|---:|
| Marginal wall per run | 902,847 ns | 903,364 ns | +0.116% [−0.998%, +1.444%] |
| Marginal cycles per run | 3,646,373 | 3,663,622 | +0.751% [−0.730%, +1.679%] |
| Marginal instructions per run | 20,669,846 | 20,462,208 | −1.015% [−1.040%, −0.974%] |
| Work max RSS median | 116,391,936 B | 90,767,360 B | −21.983% [−22.044%, −21.948%] |
| Setup-only max RSS median | 65,339,392 B | 51,544,064 B | −13,795,328 B (−21.12%) |

The paired wall and cycle intervals include zero, so this screen detects neither a speed gain nor a speed regression on Splay. The RSS reduction is clear. This supports proceeding to the all-eight fixed-work guard once the host is clean; it does not establish a stock Score result or authorize retaining the layout by itself. The stock retake for trunk `4a3925c7b` is still pending.
