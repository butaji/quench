#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cargo_config="$root/.cargo/config.toml"
probe=$(mktemp -d "${TMPDIR:-/tmp}/quench-profile-contract.XXXXXX")
trap 'rm -rf "$probe"' EXIT

# A score artifact has one declared Cargo policy. Local configuration may
# enforce diagnostics, but must not silently rewrite code generation.
if awk '
  $0 == "[profile.release]" { found = 1 }
  found && /^\[/ && $0 != "[profile.release]" { exit }
  found { exit 0 }
  END { exit found ? 0 : 1 }
' "$cargo_config"; then
  printf 'profile contract failed: .cargo/config.toml must not override [profile.release]\n' >&2
  exit 1
fi

if grep -Eq 'target-(cpu|feature)[[:space:]]*=' "$cargo_config"; then
  printf 'profile contract failed: production config must remain portable (target CPU features belong to recorded benchmark invocations)\n' >&2
  exit 1
fi

log="$probe/cargo.log"
(cd "$root" && CARGO_TARGET_DIR="$probe/target" cargo build -p quench-runtime --profile production -vv >"$log" 2>&1)

line=$(grep 'rustc --crate-name quench_runtime ' "$log" | tail -n 1 || true)
if [[ -z "$line" ]]; then
  printf 'profile contract failed: Cargo did not expose quench_runtime rustc invocation\n' >&2
  exit 1
fi
for flag in '-C opt-level=3' '-C panic=abort' '-C linker-plugin-lto' '-C codegen-units=1'; do
  if [[ "$line" != *"$flag"* ]]; then
    printf 'profile contract failed: effective production rustc invocation lacks %s\n' "$flag" >&2
    exit 1
  fi
done

printf 'production profile contract: ok\n'
