# Cached JS equality artifact screen

This screen measured a temporary change that retained a compiled JavaScript `equals` artifact and parameter count through a numeric hash-map scan, avoiding repeated tier-table lookups. It did not demonstrate an improvement and the change was discarded.

## Setup

- Host: Linux x86_64, kernel 6.18.55, 4 CPU quota, 16 GiB memory cgroup.
- Corpus: JetStream 3 pinned at `06785cf861ac44855f168cbbe829278c2802e6de`.
- Runtime: `quench-node` production profile, JIT enabled.
- Candidate binary SHA-256: `31b6647591f4556b718f2e6d10e9f83c42e8bbf9feb1d981fb24583d478103cf`.
- Control binary SHA-256: `49848563687bf496cf38be61e71022f6e8e62f79f399ea9b4a28190100ddd4ed`.
- Control and candidate were built from source revision `695928aa28e008d37505dadf1091a5c777332f8e`; candidate had the temporary working-tree change.
- Each report uses 11 rounds for one fixture (`hash-map`). Reports are valid and outputs equal, but they are not full-suite qualification runs.

## Results

| Build | Quench median Score | Quench median peak RSS | Bun median Score | Node median Score |
| --- | ---: | ---: | ---: | ---: |
| Control | 13.47 | 36.2 MiB | 258.72 | 211.72 |
| Candidate | 10.40 | 37.4 MiB | 199.16 | 163.17 |

Comparator performance and Quench were noisy between sessions. This one fixture and sequential sessions do not identify a causal regression or improvement. The candidate is not accepted: it showed no score gain and slightly higher median RSS. The existing JIT remains far from its JetStream score target.

## Commands

Both runs used `target/production/quench-bench --fixtures hash-map --suite jetstream3 --jit --runs 11 --timeout-ms 600000`, with the relevant binary passed via `--quench`, installed Node v26.10.0 and Bun 1.4.2 via `--node` and `--bun`, and output paths `control-11.json` / `candidate-11.json`. The unsupported QJS engine was disabled using `/bin/true`.

Raw runner reports are retained in this directory. The temporary binaries are retained only at `/tmp/quench-jit-artifact-cache-screen/` for the current workspace session.
