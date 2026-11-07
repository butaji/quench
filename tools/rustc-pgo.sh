#!/bin/sh
# Cargo's rustc wrapper (.cargo/config.toml `build.rustc-wrapper`): every optimized
# (`opt-level=3`) x86-64 Linux compilation uses the trained profile
# crates/quench-node/pgo/x86_64-linux.profdata, so a plain `cargo build --release` is the
# profile-guided build. tools/pgo-build.sh trains the profile; while it builds the
# instrumented binary it names the raw profile directory in QUENCH_PGO_GENERATE.
#
# A profile trained on an older revision still applies: functions whose control flow changed
# since then lose only their own counts (hence no mismatch warnings).
#
# Indirect call promotion stays off: the lane's dispatches are indirect tail calls between
# handlers, and promoting one inlines a large handler into a small one, whose every dispatch
# then pays the large one's prologue (Crypto and NavierStokes ran 38% more instructions).
rustc=$1
shift
optimized=false
host_target=true
previous=
for argument in "$@"; do
    case "$argument" in
        opt-level=3) [ "$previous" = -C ] && optimized=true ;;
        -Copt-level=3) optimized=true ;;
    esac
    if [ "$previous" = --target ] && [ "$argument" != x86_64-unknown-linux-gnu ]; then
        host_target=false
    fi
    previous=$argument
done
if [ "$optimized" = true ] && [ "$host_target" = true ] && [ "$(uname -sm)" = "Linux x86_64" ]; then
    if [ -n "${QUENCH_PGO_GENERATE:-}" ]; then
        exec "$rustc" "$@" "-Cprofile-generate=$QUENCH_PGO_GENERATE"
    fi
    profile="$(cd "$(dirname "$0")/.." && pwd)/crates/quench-node/pgo/x86_64-linux.profdata"
    if [ -f "$profile" ]; then
        exec "$rustc" "$@" "-Cprofile-use=$profile" -Cllvm-args=-disable-icp -Cllvm-args=-pgo-warn-mismatch=false
    fi
fi
exec "$rustc" "$@"
