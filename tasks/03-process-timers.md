# Stage 03 — Process and timers

Complete `process` observable state and mutation, argv/env/stdio, exit and signal ordering, nextTick, timers and promises, immediates, ref/unref/refresh/dispose, and `performance`/`perf_hooks` marks, measures, observers, and timerify. Preserve event-loop ordering and error propagation; do not fake unsupported state.

Use matching upstream process/timer/perf fixtures plus existing stages 366, 470, 2607, 2613. Acceptance: ordering, cancellation, validation, exit codes, and callback/promise parity pass through the real runner.
