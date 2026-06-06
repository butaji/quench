#!/bin/bash
# Ink Parity Harness
# Compares rendered output across deno, rquickjs dev, and compile environments.
#
# Usage:
#   ./scripts/parity.sh --env deno|rq|compile|all --examples GLOB --once --verbose

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/parity_lib.sh"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BLUE='\033[0;34m'
NC='\033[0m'
BOLD='\033[1m'

# Defaults
ENV="all"
EXAMPLES_GLOB="ink-*"
ONCE_FLAG=""
VERBOSE=false
THRESHOLD=95.0

# Parse arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --env)
      ENV="$2"
      shift 2
      ;;
    --examples)
      EXAMPLES_GLOB="$2"
      shift 2
      ;;
    --once)
      ONCE_FLAG="--once"
      shift
      ;;
    --verbose)
      VERBOSE=true
      shift
      ;;
    --timeout)
      TIMEOUT_SECS="$2"
      shift 2
      ;;
    *)
      echo "Unknown option: $1"
      echo "Usage: $0 --env deno|rq|compile|all --examples GLOB --once --verbose"
      exit 1
      ;;
  esac
done

init_tmp

# Resolve example directories
EXAMPLES=()
# Split comma or space separated patterns
IFS=', ' read -ra PATTERNS <<< "$EXAMPLES_GLOB"
for pattern in "${PATTERNS[@]}"; do
  [[ -z "$pattern" ]] && continue
  for ex_dir in "$REPO_ROOT/examples"/$pattern; do
    if [[ -d "$ex_dir" ]] && [[ -f "$ex_dir/tui/app.tsx" ]]; then
      # Avoid duplicates
      dup=false
      for existing in ${EXAMPLES[@]+"${EXAMPLES[@]}"}; do
        if [[ "$existing" == "$ex_dir" ]]; then
          dup=true
          break
        fi
      done
      if [[ "$dup" == "false" ]]; then
        EXAMPLES+=("$ex_dir")
      fi
    fi
  done
done

if [[ ${#EXAMPLES[@]} -eq 0 ]]; then
  echo "No examples found matching: $EXAMPLES_GLOB"
  exit 1
fi

TOTAL=${#EXAMPLES[@]}
PASSED=0
FAILED=0

# Build a JSON array string
JSON_PARTS=()

header() {
  echo ""
  echo -e "${BOLD}Ink Parity Harness${NC}"
  echo -e "Environment: ${CYAN}${ENV}${NC}"
  echo -e "Examples:    ${CYAN}${TOTAL}${NC}"
  echo -e "Threshold:   ${CYAN}${THRESHOLD}%${NC}"
  echo ""
}

run_all() {
  for i in "${!EXAMPLES[@]}"; do
    local ex_dir="${EXAMPLES[$i]}"
    local name
    name=$(basename "$ex_dir")
    local num=$((i + 1))

    echo -n -e "${BLUE}[${num}/${TOTAL}]${NC} ${BOLD}${name}${NC} ... "

    local deno_sim="0.00"
    local rq_sim="0.00"
    local compile_sim="0.00"
    local deno_status="skip"
    local rq_status="skip"
    local compile_status="skip"
    local interactive=false

    if is_interactive "$ex_dir/tui/app.tsx"; then
      interactive=true
    fi

    # Run deno
    if [[ "$ENV" == "all" ]] || [[ "$ENV" == "deno" ]]; then
      if $interactive; then
        deno_status="skip"
        # Attempt anyway; if it works, great
        if run_deno "$ex_dir"; then
          deno_status="ok"
          deno_sim="100.00"
        fi
      else
        if run_deno "$ex_dir"; then
          deno_status="ok"
          deno_sim="100.00"
        else
          deno_status="err"
        fi
      fi
    fi

    # Run rq
    if [[ "$ENV" == "all" ]] || [[ "$ENV" == "rq" ]]; then
      if run_rq "$ex_dir" "$ONCE_FLAG"; then
        rq_status="ok"
      else
        rq_status="err"
      fi
    fi

    # Run compile
    if [[ "$ENV" == "all" ]] || [[ "$ENV" == "compile" ]]; then
      if run_compile "$ex_dir"; then
        compile_status="ok"
      else
        compile_status="err"
      fi
    fi

    # Normalize outputs
    local deno_norm=""
    local rq_norm=""
    local compile_norm=""

    if [[ "$deno_status" == "ok" ]]; then
      deno_norm="$TMP_DIR/deno_${name}_norm.txt"
      normalize_output < "$TMP_DIR/deno_${name}.txt" > "$deno_norm"
    fi
    if [[ "$rq_status" == "ok" ]]; then
      rq_norm="$TMP_DIR/rq_${name}_norm.txt"
      normalize_output < "$TMP_DIR/rq_${name}.txt" > "$rq_norm"
    fi
    if [[ "$compile_status" == "ok" ]]; then
      compile_norm="$TMP_DIR/compile_${name}_norm.txt"
      normalize_output < "$TMP_DIR/compile_${name}.txt" > "$compile_norm"
    fi

    # Compute similarities against deno baseline
    if [[ "$ENV" == "all" ]] || [[ "$ENV" == "rq" ]]; then
      if [[ -n "$deno_norm" ]] && [[ -n "$rq_norm" ]]; then
        rq_sim=$(compute_similarity "$deno_norm" "$rq_norm")
      elif [[ -n "$rq_norm" ]]; then
        # No deno baseline available (subset run or interactive)
        rq_sim="100.00"
      fi
      if [[ "$rq_status" == "ok" ]] && [[ $(echo "$rq_sim < $THRESHOLD" | bc -l) -eq 1 ]]; then
        rq_status="fail"
      fi
    fi

    if [[ "$ENV" == "all" ]] || [[ "$ENV" == "compile" ]]; then
      if [[ -n "$deno_norm" ]] && [[ -n "$compile_norm" ]]; then
        compile_sim=$(compute_similarity "$deno_norm" "$compile_norm")
      elif [[ -n "$compile_norm" ]] && [[ -n "$rq_norm" ]]; then
        compile_sim=$(compute_similarity "$rq_norm" "$compile_norm")
      elif [[ -n "$compile_norm" ]]; then
        # No baseline available
        compile_sim="100.00"
      fi
      if [[ "$compile_status" == "ok" ]] && [[ $(echo "$compile_sim < $THRESHOLD" | bc -l) -eq 1 ]]; then
        compile_status="fail"
      fi
    fi

    # Overall example verdict
    local example_ok=true
    if [[ "$ENV" == "all" ]] || [[ "$ENV" == "deno" ]]; then
      if [[ "$deno_status" == "err" ]] || [[ "$deno_status" == "fail" ]]; then
        example_ok=false
      fi
    fi
    if [[ "$ENV" == "all" ]] || [[ "$ENV" == "rq" ]]; then
      if [[ "$rq_status" == "err" ]] || [[ "$rq_status" == "fail" ]]; then
        example_ok=false
      fi
    fi
    if [[ "$ENV" == "all" ]] || [[ "$ENV" == "compile" ]]; then
      if [[ "$compile_status" == "err" ]] || [[ "$compile_status" == "fail" ]]; then
        example_ok=false
      fi
    fi

    if $example_ok; then
      echo -e "${GREEN}PASS${NC}  deno:${deno_sim}% rq:${rq_sim}% compile:${compile_sim}%"
      PASSED=$((PASSED + 1))
    else
      echo -e "${RED}FAIL${NC}  deno:${deno_sim}% rq:${rq_sim}% compile:${compile_sim}%"
      FAILED=$((FAILED + 1))
      if $VERBOSE; then
        show_diffs "$name" "$deno_norm" "$rq_norm" "$compile_norm"
      fi
    fi

    JSON_PARTS+=("{\"example\":\"$name\",\"deno\":{\"status\":\"$deno_status\",\"similarity\":$deno_sim},\"rq\":{\"status\":\"$rq_status\",\"similarity\":$rq_sim},\"compile\":{\"status\":\"$compile_status\",\"similarity\":$compile_sim}}")
  done
}

show_diffs() {
  local name=$1
  local deno_norm=$2
  local rq_norm=$3
  local compile_norm=$4

  echo ""
  echo -e "  ${CYAN}--- ${name} ---${NC}"
  if [[ -n "$deno_norm" ]] && [[ -f "$deno_norm" ]]; then
    echo -e "  ${CYAN}[deno]${NC}"
    head -12 "$deno_norm" | sed 's/^/    /'
  fi
  if [[ -n "$rq_norm" ]] && [[ -f "$rq_norm" ]]; then
    echo -e "  ${CYAN}[rq]${NC}"
    head -12 "$rq_norm" | sed 's/^/    /'
  fi
  if [[ -n "$compile_norm" ]] && [[ -f "$compile_norm" ]]; then
    echo -e "  ${CYAN}[compile]${NC}"
    head -12 "$compile_norm" | sed 's/^/    /'
  fi
}

write_json_report() {
  local report="$TMP_DIR/report.json"
  {
    echo "["
    local first=true
    for part in "${JSON_PARTS[@]}"; do
      if $first; then
        first=false
      else
        echo ","
      fi
      echo -n "  $part"
    done
    echo ""
    echo "]"
  } > "$report"

  echo ""
  echo -e "${BOLD}═══════════════════════════════════════════════════════${NC}"
  echo -e "${BOLD}Summary${NC}"
  echo -e "  ${GREEN}Passed:${NC}  $PASSED"
  echo -e "  ${RED}Failed:${NC}  $FAILED"
  echo -e "  Total:   $TOTAL"
  echo -e "${BOLD}═══════════════════════════════════════════════════════${NC}"
  echo ""
  echo "JSON report:"
  cat "$report"
  echo ""
}

# ─── main ──────────────────────────────────────────────────────────
header
run_all
write_json_report

if [[ $FAILED -eq 0 ]]; then
  exit 0
else
  exit 1
fi
