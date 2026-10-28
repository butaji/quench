# Empty AsyncLocalStorage snapshot screen

The Promise job capture path calls `inherit_stores` for every job. A small
candidate change returned immediately when the entire `local_stores` map was
empty, avoiding collection into an empty temporary vector. Both the baseline
and candidate used uninstrumented production binaries and validated all three
rounds for Promise and Async.

| Fixture | Baseline score / RSS | Candidate score / RSS |
| --- | ---: | ---: |
| `doxbee-promise` | 3.07 / 170,000,384 B | 3.52 / 170,131,456 B |
| `doxbee-async` | 5.19 / 257,392,640 B | 4.73 / 257,564,672 B |

The screens were sequential, and peer score distributions changed substantially
between them (for example Bun's Async median moved 293.50 to 125.48). The
opposite Quench score movements and effectively flat RSS do not establish a
win. The candidate source change was reverted; do not treat these results as a
performance improvement. A paired screen would be needed before retrying it.

The raw reports include per-round values, executable hashes, and source
provenance in `off-3round.json` and `on-3round.json`.
