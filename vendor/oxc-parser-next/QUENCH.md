# OXC parser stack integration

This is OXC `oxc_parser` 0.150.0 from the pinned crates.io archive.
Archive SHA256: `44083f63120e1abe371cddd4eb209a580ab8490c2e5b42cacc11e7fe6fd1e7c2`.
Upstream revision: `5a6e37e5cf895143a5b34050c50109c46e2ae96a`.
Upstream path: `crates/oxc_parser`. The upstream MIT license is retained.

OXC continues to own grammar and syntax. Quench adds the shared `quench-stack`
reservation at recursive expression, statement and binding transitions. Exhaustion
uses OXC's existing fatal-error path and produces a distinct `ParserReturn`
projection. Syntax checkpoints retain resource exhaustion; rewinding cannot
resume a parser after the budget has been exhausted. No source pre-scan or guest
syntax recognition is added.

`QUENCH.patch` records the source changes against the archive as a zero-context
unified diff (`git apply --unidiff-zero`). Cargo's patch
selects this version only; the legacy OXC 0.47 parser still needs separate coverage.
