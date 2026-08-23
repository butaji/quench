#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cargo_toml="$root/Cargo.toml"
cargo_config="$root/.cargo/config.toml"

section_lines() {
  local file=$1
  local section=$2
  awk -v section="$section" '
    $0 == section { in_section = 1; next }
    /^\[/ { in_section = 0 }
    in_section { print }
  ' "$file"
}

require_profile_line() {
  local file=$1
  local section=$2
  local expected=$3
  if ! section_lines "$file" "$section" | grep -Fqx "$expected"; then
    printf 'profile contract failed: %s is missing from %s in %s\n' \
      "$expected" "${file#"$root/"}" "$section" >&2
    exit 1
  fi
}

forbid_profile_line() {
  local file=$1
  local section=$2
  local forbidden=$3
  if section_lines "$file" "$section" | grep -Fqx "$forbidden"; then
    printf 'profile contract failed: %s must not appear in %s in %s\n' \
      "$forbidden" "${file#"$root/"}" "$section" >&2
    exit 1
  fi
}

require_line() {
  local file=$1
  local expected=$2
  if ! grep -Fqx "$expected" "$file"; then
    printf 'profile contract failed: %s is missing from %s\n' "$expected" "${file#"$root/"}" >&2
    exit 1
  fi
}


# The shipped release profile is the production contract: size-oriented codegen,
# whole-program optimization, deterministic single-unit codegen, and no unwind
# or symbol payload in the executable.
require_profile_line "$cargo_toml" '[profile.release]' 'opt-level = "z"'
require_profile_line "$cargo_toml" '[profile.release]' 'lto = "fat"'
require_profile_line "$cargo_toml" '[profile.release]' 'codegen-units = 1'
require_profile_line "$cargo_toml" '[profile.release]' 'strip = "symbols"'
require_profile_line "$cargo_toml" '[profile.release]' 'panic = "abort"'
require_profile_line "$cargo_toml" '[profile.release-thin]' 'inherits = "release"'
require_profile_line "$cargo_toml" '[profile.release-thin]' 'lto = "thin"'

# .cargo/config.toml is an intentional, executable override: Cargo merges it
# after the manifest, so assert the effective release values in that section,
# rather than accepting matching text in another profile.
require_profile_line "$cargo_config" '[profile.release]' 'opt-level = 3'
require_profile_line "$cargo_config" '[profile.release]' 'lto = true'
require_profile_line "$cargo_config" '[profile.release]' 'codegen-units = 1'
require_profile_line "$cargo_config" '[profile.release]' 'panic = "abort"'
forbid_profile_line "$cargo_config" '[profile.release]' 'incremental = true'
forbid_profile_line "$cargo_config" '[profile.release]' 'debug = true'

if grep -Eq 'target-(cpu|feature)[[:space:]]*=' "$cargo_config"; then
  printf 'profile contract failed: production config must remain portable (target CPU features belong to benchmarks only)\n' >&2
  exit 1
fi

printf 'production profile contract: ok\n'
