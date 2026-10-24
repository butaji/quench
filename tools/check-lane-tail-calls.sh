#!/usr/bin/env bash
# Lists the lane handlers of a quench-node binary whose dispatch of the next record is a call
# instead of a sibling-call jump.
#
# usage: tools/check-lane-tail-calls.sh BINARY [HANDLER...]
#
# A lane handler ends by dispatching its successor's handler (`next!`, `sibling!`). On the
# builds that form sibling calls (`quench_lane_tail_calls`) that dispatch must be a jump: a
# call nests a native frame per record run, which grows the stack until the safepoint floor
# sends the run back to the lane loop. LLVM forms the jump only when no stack temporary of
# the handler escapes. A helper the handler inlines that takes a local's address, or returns
# a value too wide for registers, prevents it, and profile-guided inlining can make that
# happen to a hot handler in one build and not the next (one such `IncDec` handler made
# NavierStokes 17% slower at an equal instruction count). The check reads symbols, so run
# it on a binary that keeps them (`production`, `profiling`). It prints each handler with an
# indirect call and how many it has; the HANDLER names given must have none, and the exit
# status is 1 when one does.
set -euo pipefail
binary="$1"
shift
report="$(objdump -d --no-show-raw-insn -C "$binary" | awk '
    /^[0-9a-f]+ <.*>:$/ {
        name = $0
        sub(/^[0-9a-f]+ </, "", name)
        sub(/>:$/, "", name)
        handler = (name ~ /dispatch_fast::lane_/)
    }
    handler && /call +\*(0x[0-9a-f]+)?\(%r[a-z0-9]+\)$/ { calls[name]++ }
    END { for (name in calls) printf "%d %s\n", calls[name], name }
' | sort -k2)"
printf '%s\n' "$report"
status=0
for required in "$@"; do
    if grep -qF -- "dispatch_fast::$required::<" <<<"$report"; then
        printf 'lane handler dispatches with a call: %s\n' "$required" >&2
        status=1
    fi
done
exit "$status"
