# QuickSort indexed-offset reuse A/B

This experiment screens the default-off `QUENCH_WASM_JIT_INDEXED_OFFSET_REUSE`
candidate on JetStream 3 `quicksort-wasm`. The candidate maintains an exact
i32-times-four companion and replaces shift-by-two values in structurally
matching functions. Existing wrapped address formation and memory guards are
unchanged.

The first screen is predeclared at three paired rounds. Continue to eleven
rounds only if the on/off paired Score ratio is at least 1.05 with its 95%
interval above a tie, and peak RSS increases by at most 0.5 MiB. The runner
always marks this diagnostic as non-qualifying. A confirmed local improvement
still does not count as a peer win unless the same report also puts Quench
above both Bun and Node on Score and below both on peak RSS.

The focused regression type-checks. A public-API smoke run matched the
interpreter, compiled-off, and compiled-on paths on ordinary indices,
modulo-wrapped indices, and out-of-bounds traps. The full unit-test harness
could not finish compiling within the container's memory limit; no assertions
from that harness ran.

Run commands, source/binary hashes, raw samples, paired analysis, and the
screen outcome will be recorded here after the candidate build is measured.
