# Stage 06 — Networking and process workers

Finish `net`, `dns`, `dgram`, Unix sockets, lookup options, connection lifecycle, and then `child_process` and `cluster` where host process capabilities permit. Reuse stream/resource state machines and event-loop readiness; keep OS effects at the host boundary.

Run upstream net/dns/dgram/child-process fixtures plus focused stages 2257–2318, 2565–2575, 2506–2516. Acceptance: overload validation, address identity, half-close, abort/timeout, callback order, stdio, spawn errors, and cleanup match Node.
