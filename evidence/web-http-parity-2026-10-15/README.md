# HTTP parity repairs — 2026-10-15

Source: `99dd33963`, based on shared integration `b8917f7e6`. Binary identities
are recorded in `qualification.json`. The official oracle is Node v26.12.0;
its release source is pinned and hash checked. No fixture source was modified.

## Repairs

- Preserve Writable error-event delivery for response writes after end.
- Forward request/get URL, options and callback overloads.
- Reuse net.Server inheritance and canonical listener handles for ref/unref.
- Apply Node’s method-dependent default request-body framing.
- Reject invalid trailers and malformed transfer codings, preserving request
  dispatch order and an application response already committed to the wire.

The production changes use shared transport ownership and framing policy;
there is no fixture recognition or benchmark-specific behavior.

## Checks

- Workspace/all-target check passes.
- Runtime library: 618 passed.
- Framework core: 43 passed.
- Five unchanged upstream fixtures in the `web-parity` profile pass.
- Six HTTP differential tests pass against official Node, including seven
  transfer-coding cases with event ordering and default error wire bytes.
- Ten focused fixtures pass through the real CLI on both runtimes; the
  previous Quench result was five pass, three failures and two timeouts.
  Raw stdout/stderr are retained. Exit status alone is not full conformance.
- All nine framework/runtime combinations pass correctness verification in
  the pinned SaltyAom suite, including the complete video checksum. Node uses
  `--jitless`, Bun reports `useJIT=false`, Quench uses the interpreter.

The web runner records a clean source commit and the frozen candidate SHA256.
The instruction guard compares the previous published integration binary with
this candidate; it includes other merged lane changes, so it does not isolate
attribution to the HTTP changes. Counts are iteration-build instructions,
not production throughput or RSS measurements.

## Remaining scope

This is a functional repair milestone, not total Node parity or a benchmark
win. Custom HTTP clientError handling, real request socket behavior, HTTP/2
transport and broader Node/npm APIs remain incomplete. The existing high-load
socket timeouts have not been repaired or scored as successes. Optimization
remains behind the user’s parity requirement. Linear: TOD-133, TOD-134, TOD-135.

## Shared integration

The subsequent merge `03ea582d2` includes current shared TLS/Fetch changes.
Its workspace check passes. The first full API run overlapped Callgrind and
finished 241/248; seven startup/deadline-sensitive checks failed. That complete
failed log is retained. All seven pass serially without CPU instrumentation and with unchanged
deadlines; raw logs from both runs are retained. This is not described as a
single clean 248-test run. The integrated executable also passes all ten paired
real-CLI fixtures and all nine no-JIT framework correctness combinations.
The all-eight guard qualifies frozen source `99dd33963`, not this later merge.

The merged tree also retains a complete historical stable-source sweep in
`../task20-node-latest-stable-duplex-full-2026-10-15.json`. It predates these
HTTP repairs and is diagnostic, not a full conformance result for this candidate.
Our incomplete short-timeout refresh was stopped and is not used for counts.

## Instruction finding

All 32 baseline/candidate counts completed. DeltaBlue (+2.51%), RegExp (+1.72%)
and Splay (+2.75%) exceed the 1% per-run guard. The other five are within 1%.
Startup changes range from -4.05 million to +56.06 million instructions versus
this already-costly baseline. Raw counts, normalized values and summary are
retained. TOD-135 records the regressions; this functional milestone makes no
performance claim and does not waive the parity prerequisite.
