# Initial native net lifecycle exposes duplicate HTTP listening state

Frozen source 409e83ee9aade9085e5113f2b60e4be0cbef76ce fails six of 386 serial Node v26.12.0 API comparisons: 380 pass, six fail, exit 101 in 260.45 seconds. Source/oracle/CLI/runner/test identities remain unchanged across the run. This is a failed qualification, not parity or performance evidence.

Four HTTP response/wire tests fail while constructing a server; both TCP syscall tests also fail in their HTTP cases. The new Node-compatible inherited listening getter derives from _handle. Legacy HTTP code still writes a separate listening boolean, producing "cannot install shared http property listening". The subsequent correction removes those duplicate writes and clears the actual handle on close; that later source and its results are excluded here.

Focused native lifecycle, DNS iterator/GC and capture-reset cases pass before this full failure. Earlier first-binary callback-name mismatch, missing repeated-close callback, and occupied capture-slot reset failure remain diagnostic records. Native function/source/host-frame fidelity and broader Node behavior remain open. No total parity, optimization or web win is claimed.
