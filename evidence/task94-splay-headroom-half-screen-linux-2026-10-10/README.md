# Splay 0.5 headroom screen on Linux

This is a rejected policy screen, not a retained production setting. It changed
only `LARGE_HEAP_GC_HEADROOM` from 1/1 to 1/2 while keeping
`FULL_COLLECTION_GROWTH` at 1/1. It was measured with Rust 1.99.0, the pinned
V8-v7 fixture, Node v26.10.0 `--jitless`, Bun 1.4.2 no-JIT, and QuickJS
2026-06-04. Candidate binary SHA-256:
`0a876280e70c297fb391f09ac584887f947419e0a49f1c2afd4838ea3bdc3fc2`.

The three-round stock diagnostic is output-equal (3/3) and records Quench Score
2,000 vs Node 5,036; Quench max RSS 123,146,240 B vs Bun 60,272,640 B. It is too
short for a qualification or a stable score claim, and both bars remain far
away.

The paired fixed-work comparison against the clean `origin/v2` production
binary (SHA-256
`e086bf485b76accb09972e0596075aec37dc718c909f183528841a94aeffbf50`) is valid
and output-equal for 11/11 rounds. Linux wait4 lacks cycles and instructions;
the contention detector consequently reports 0/11 clean rounds and no speed
median. RSS medians are Quench 122,941,440 B for the 1/2 candidate versus
119,721,984 B on trunk (+3,219,456 B); setup-only RSS is 67,747,840 B versus
66,048,000 B (+1,699,840 B).

The 1/2 setting reduces candidate RSS relative to the separate 1/1 stock
screen, but that comparison is unpaired, and the paired result still uses more
RSS than trunk. It also does not approach either Splay bar. The experiment is
reverted; no timing win is claimed and the trunk-owned headroom policy remains
unchanged.
