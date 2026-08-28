#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cargo_config="$root/.cargo/config.toml"
probe=$(mktemp -d "${TMPDIR:-/tmp}/quench-profile-contract.XXXXXX")
trap 'rm -rf "$probe"' EXIT

# Cargo merges user configuration after this repository. The local pin must
# therefore stay limited to the manifest's fat-LTO contract.
if ! awk '
  $0 == "[profile.release]" { found = 1; next }
  found && /^\[/ { exit }
  found && $0 == "lto = \"fat\"" { ok = 1 }
  END { exit ok ? 0 : 1 }
' "$cargo_config"; then
  printf 'profile contract failed: .cargo/config.toml must pin release fat LTO\n' >&2
  exit 1
fi

if grep -Eq 'target-(cpu|feature)[[:space:]]*=' "$cargo_config"; then
  printf 'profile contract failed: production config must remain portable (target CPU features belong to recorded benchmark invocations)\n' >&2
  exit 1
fi

log="$probe/cargo.log"
(cd "$root" && CARGO_TARGET_DIR="$probe/target" cargo build -p quench-node --profile production -vv >"$log" 2>&1)

line=$(grep 'rustc --crate-name quench_node ' "$log" | tail -n 1 || true)
if [[ -z "$line" ]]; then
  printf 'profile contract failed: Cargo did not expose quench_node rustc invocation\n' >&2
  exit 1
fi
for flag in '-C opt-level=3' '-C panic=abort' '-C lto' '-C codegen-units=1'; do
  if [[ "$line" != *"$flag"* ]]; then
    printf 'profile contract failed: effective production rustc invocation lacks %s\n' "$flag" >&2
    exit 1
  fi
done
if [[ "$line" == *'-C lto=thin'* ]]; then
  printf 'profile contract failed: effective production rustc invocation uses Thin LTO\n' >&2
  exit 1
fi

printf 'production profile contract: ok\n'
