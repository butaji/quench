#!/bin/bash
# Shared functions for the Ink parity harness.

set -uo pipefail

# SCRIPT_DIR_FULL is scripts/lib/
SCRIPT_DIR_FULL="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# REPO_ROOT is two levels up from scripts/lib/ (scripts/, then project root)
REPO_ROOT="$(cd "$SCRIPT_DIR_FULL/../.." && pwd)"
SCRIPT_LIB_DIR="$SCRIPT_DIR_FULL"
RUNTS_BIN="${RUNTS_BIN:-$REPO_ROOT/target/debug/runts}"
PYTHON_DIFF="$SCRIPT_LIB_DIR/symbol_diff.py"

TMP_DIR=""
TIMEOUT_SECS=5

init_tmp() {
  TMP_DIR=$(mktemp -d "/tmp/runts_parity_$$_XXXX")
  # shellcheck disable=SC2064
  trap 'rm -rf "$TMP_DIR"' EXIT
}

# Strip ANSI escape sequences, build noise, and trailing whitespace.
normalize_output() {
  sed 's/\x1b\[[0-9;]*[a-zA-Z]//g' \
    | sed \
      -e '/^DEBUG /d' \
      -e '/^INFO /d' \
      -e '/^[0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}T/d' \
      -e '/^Starting development server/d' \
      -e '/^Building for production/d' \
      -e '/^Found [0-9]\+ TypeScript files/d' \
      -e '/^Found [0-9]\+ routes/d' \
      -e '/^Building with cargo/d' \
      -e '/^Build complete/d' \
      -e '/^Binary:/d' \
      -e '/^Size:/d' \
      -e '/^   Created/d' \
      -e '/^   Compiling/d' \
      -e '/^    Finished/d' \
      -e '/^   Running/d' \
      -e '/^  Binary/d' \
      -e '/^error:.*warning:/d' \
      -e '/^warning:/d' \
      -e '/^$/d' \
    | sed 's/[[:space:]]*$//' \
    | tr -d '\r'
}

# True if the example source uses interactive hooks.
is_interactive() {
  local src="$1"
  grep -qE 'useInput|useFocus|useStdin' "$src" 2>/dev/null
}

# Run a command with a timeout (macOS-compatible).
run_with_timeout() {
  local secs=$1
  shift
  local out="$TMP_DIR/timeout_out_$$.txt"

  "$@" > "$out" 2>&1 &
  local pid=$!

  local count=0
  while [[ $count -lt $((secs * 2)) ]]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      break
    fi
    sleep 0.5
    count=$((count + 1))
  done

  if kill -0 "$pid" 2>/dev/null; then
    kill -9 "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi

  cat "$out" 2>/dev/null || true
}

# Run deno and capture stdout. Returns 0 on success.
run_deno() {
  local ex_dir=$1
  local name
  name=$(basename "$ex_dir")
  local out="$TMP_DIR/deno_${name}.txt"
  local err="$TMP_DIR/deno_${name}_err.txt"

  if [[ ! -f "$ex_dir/main.tsx" ]]; then
    echo "<NO_MAIN>" > "$out"
    return 1
  fi

  deno run -A "$ex_dir/main.tsx" > "$out" 2> "$err" &
  local pid=$!

  local count=0
  while [[ $count -lt $((TIMEOUT_SECS * 2)) ]]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      break
    fi
    sleep 0.5
    count=$((count + 1))
  done

  if kill -0 "$pid" 2>/dev/null; then
    kill -9 "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi

  if grep -q "Raw mode is not supported" "$out" 2>/dev/null; then
    echo "<INTERACTIVE>" > "$out"
    return 1
  fi

  if grep -qE 'TypeError|ReferenceError|SyntaxError|error:' "$err" 2>/dev/null; then
    echo "<ERROR>" > "$out"
    return 1
  fi

  if [[ ! -s "$out" ]]; then
    echo "<NO_OUTPUT>" > "$out"
    return 1
  fi

  return 0
}

# Run rquickjs dev and capture stdout. Returns 0 on success.
run_rq() {
  local ex_dir=$1
  local once_flag=${2:-}
  local name
  name=$(basename "$ex_dir")
  local out="$TMP_DIR/rq_${name}.txt"
  local err="$TMP_DIR/rq_${name}_err.txt"

  if [[ ! -f "$ex_dir/tui/app.tsx" ]]; then
    echo "<NO_APP>" > "$out"
    return 1
  fi

  local extra_args=()
  if [[ -n "$once_flag" ]]; then
    extra_args+=(--once)
  fi

  if is_interactive "$ex_dir/tui/app.tsx" && [[ -z "$once_flag" ]]; then
    printf 'q\n' | "$RUNTS_BIN" dev --plugin ratatui "$ex_dir" > "$out" 2> "$err" &
    local pid=$!
    local count=0
    while [[ $count -lt $((TIMEOUT_SECS * 2)) ]]; do
      if ! kill -0 "$pid" 2>/dev/null; then break; fi
      sleep 0.5
      count=$((count + 1))
    done
    if kill -0 "$pid" 2>/dev/null; then
      kill -9 "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  else
    if ! "$RUNTS_BIN" dev --plugin ratatui ${extra_args:+"${extra_args[@]}"} "$ex_dir" > "$out" 2> "$err"; then
      if [[ ! -s "$out" ]]; then
        echo "<ERROR>" > "$out"
        return 1
      fi
    fi
  fi

  if grep -qE 'panicked|thread .* panicked' "$err" 2>/dev/null; then
    echo "<PANIC>" > "$out"
    return 1
  fi

  if [[ ! -s "$out" ]]; then
    echo "<NO_OUTPUT>" > "$out"
    return 1
  fi

  return 0
}

# Run compile path and capture stdout. Returns 0 on success.
run_compile() {
  local ex_dir=$1
  local name
  name=$(basename "$ex_dir")
  local out="$TMP_DIR/compile_${name}.txt"
  local err="$TMP_DIR/compile_${name}_err.txt"

  if [[ ! -f "$ex_dir/tui/app.tsx" ]]; then
    echo "<NO_APP>" > "$out"
    return 1
  fi

  rm -rf "$ex_dir/.runts" "$ex_dir/target" 2>/dev/null || true

  RUNTS_KEEP_BUILD=1 "$RUNTS_BIN" build --release --plugin ratatui "$ex_dir" \
    > "$TMP_DIR/build_${name}.log" 2>&1
  if [[ $? -ne 0 ]]; then
    echo "<BUILD_ERR>" > "$out"
    return 1
  fi

  local bin=""
  for dir in "$ex_dir/target/release" "$ex_dir/.runts/build/target/release"; do
    if [[ -x "$dir/runts-app" ]]; then
      bin="$dir/runts-app"
      break
    fi
  done

  if [[ -z "$bin" ]]; then
    echo "<NO_BINARY>" > "$out"
    return 1
  fi

  if is_interactive "$ex_dir/tui/app.tsx"; then
    printf 'q\n' | run_with_timeout "$TIMEOUT_SECS" "$bin" > "$out" 2> "$err"
  else
    printf '\n' | run_with_timeout "$TIMEOUT_SECS" "$bin" > "$out" 2> "$err"
  fi

  if [[ ! -s "$out" ]]; then
    echo "<NO_OUTPUT>" > "$out"
    return 1
  fi

  return 0
}

# Compute similarity between two files.
compute_similarity() {
  local file1=$1
  local file2=$2
  python3 "$PYTHON_DIFF" "$file1" "$file2"
}
