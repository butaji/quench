# JSON ASCII Units experiment

This experiment changed the JSON parser to consume `Units` directly and avoid widening ASCII input before parsing. The candidate passed the runtime library suite (799/799) and JetStream's output equality check for `json-parse-inspector`.

The first candidate 3-round screen is `candidate-screen-3-rounds.json`; its Quench binary was built with the unoptimized `iteration` profile and is not comparable with the production baseline. Do not use it for score conclusions.

The valid same-profile screens use production builds and 11 rounds each:

| Binary | Score median | Peak RSS median |
| -- | --: | --: |
| Baseline | 63.78 | 91.78 MiB |
| Candidate | 59.14 | 91.50 MiB |

The candidate/baseline ratios are 0.927 score and 0.997 RSS. The score regressed 7.3%, with negligible RSS change, so the implementation was rejected and reverted. Both reports used the stock `json-parse-inspector` fixture and output equality was true. These single-fixture reports are not JetStream qualification evidence (`fixture_set_matches=false`).

The baseline binary SHA-256 is `383cd816ba60f6395f6519a1cc6d6a3d35ced9aa6507e98248a4f6f06a212eda`. The candidate binary SHA-256 is `3c3527bbf0ab5ff4739ad6315c611a6652336603f6da9f6d6682c9f8ac663c66`.
