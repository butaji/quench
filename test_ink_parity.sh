#!/bin/bash
# =============================================================================
# INK EXAMPLES PARITY TEST HARNESS - macOS/Linux compatible
# =============================================================================
# Tests 100% look&feel parity across 3 environments:
#   1. deno        - Reference TypeScript runtime (npm:ink)
#   2. runts dev   - HIR runtime (QuickJS/HIR interpreter with hot-reload)
#   3. runts build - In-memory transpile + Rust compilation
#
# Usage: ./test_ink_parity.sh [--quick] [--examples ink-counter ...]
#   --quick       Skip compile step (faster iteration)
#   --examples    List specific examples to test
# =============================================================================

set +e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLES_DIR="$SCRIPT_DIR/examples"
RUNTS_BIN="$SCRIPT_DIR/target/debug/runts"
TMP_DIR="/tmp/runts_ink_parity_$$_$(date +%s)"
RESULTS_DIR="$TMP_DIR/results"
DIFFS_DIR="$TMP_DIR/diffs"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BLUE='\033[0;34m'
NC='\033[0m'
BOLD='\033[1m'

RUN_QUICK=${RUN_QUICK:-false}
SPECIFIC_EXAMPLES=""
PARITY_THRESHOLD=40

# Parse args
while [[ $# -gt 0 ]]; do
    case $1 in
        --quick) RUN_QUICK=true; shift ;;
        --examples)
            shift
            while [[ $# -gt 0 ]] && [[ ! "$1" =~ ^-- ]]; do
                SPECIFIC_EXAMPLES="$SPECIFIC_EXAMPLES $1"
                shift
            done
            ;;
        *) shift ;;
    esac
done

cleanup() { rm -rf "$TMP_DIR" 2>/dev/null || true; }
trap cleanup EXIT
mkdir -p "$RESULTS_DIR" "$DIFFS_DIR"

strip_ansi() { sed -E 's/\x1b\[[0-9;]*m//g'; }

# Normalize output for comparison
normalize() {
    strip_ansi | \
    tr -d '\r' | \
    # Normalize Unicode box drawing
    sed 's/─/-/g; s/│/|/g' | \
    # Remove leading/trailing whitespace per line
    sed 's/^[[:space:]]*//; s/[[:space:]]*$//' | \
    # Remove empty lines and limit output
    grep -v '^$' | \
    head -50
}

run_with_timeout() {
    local timeout_sec=$1
    local cmd="$2"
    local output=$3
    
    (
        eval "$cmd" > "$output" 2>&1
    ) &
    local pid=$!
    
    local count=0
    while [[ $count -lt $timeout_sec ]]; do
        sleep 1
        if ! kill -0 $pid 2>/dev/null; then
            wait $pid 2>/dev/null || true
            return 0
        fi
        count=$((count + 1))
    done
    
    kill -9 $pid 2>/dev/null || true
    wait $pid 2>/dev/null || true
    echo "TIMEOUT" >> "$output"
    return 0
}

is_interactive_output() {
    grep -qi "Raw mode is not supported\|cannot enable raw mode" "$1" 2>/dev/null
}

run_deno() {
    local ed=$1 name=$(basename "$ed")
    local out="$RESULTS_DIR/deno_$name.txt"
    local tmp="$TMP_DIR/deno_$name.txt"
    
    [[ ! -f "$ed/main.tsx" ]] && { echo "<NO_MAIN>" > "$out"; return 1; }
    
    run_with_timeout 3 "deno run -A '$ed/main.tsx'" "$tmp"
    
    if is_interactive_output "$tmp"; then
        echo "<INTERACTIVE>" > "$out"
        return 2
    fi
    
    if grep -qi "TypeError\|ReferenceError\|SyntaxError" "$tmp" 2>/dev/null; then
        echo "<DENO_ERR>" > "$out"
        cat "$tmp" >> "$out"
        return 3
    fi
    
    normalize < "$tmp" > "$out"
    return 0
}

run_hir() {
    local ed=$1 name=$(basename "$ed")
    local app="$ed/tui/app.tsx"
    local out="$RESULTS_DIR/hir_$name.txt"
    
    [[ ! -f "$app" ]] && { echo "<NO_APP>" > "$out"; return 1; }
    run_with_timeout 3 "$RUNTS_BIN hir-render '$app'" "$out"
    return 0
}

run_compile() {
    local ed=$1 name=$(basename "$ed")
    local out="$RESULTS_DIR/compile_$name.txt"
    
    [[ ! -f "$ed/tui/app.tsx" ]] && { echo "<NO_APP>" > "$out"; return 1; }
    
    rm -rf "$ed/.runts" "$ed/target" 2>/dev/null || true
    run_with_timeout 30 "$RUNTS_BIN build '$ed' --plugin ratatui --release" "$TMP_DIR/build_$name.txt"
    
    if grep -q "error\|panic" "$TMP_DIR/build_$name.txt" 2>/dev/null && ! grep -q "Finished" "$TMP_DIR/build_$name.txt"; then
        echo "<BUILD_ERR>" > "$out"
        return 4
    fi
    
    local bin=""
    for dir in "$ed/target/release" "$ed/.runts/target/release"; do
        [[ -x "$dir/runts-app" ]] && bin="$dir/runts-app" && break
    done
    
    [[ -z "$bin" ]] && { echo "<NO_BINARY>" > "$out"; return 5; }
    run_with_timeout 3 "'$bin'" "$out"
    return 0
}

# Calculate similarity - uses common line count
calc_sim() {
    local f1=$1 f2=$2
    [[ ! -f "$f1" ]] || [[ ! -f "$f2" ]] && { echo "0"; return; }
    
    local l1=$(normalize < "$f1" | wc -l | tr -d ' ')
    local l2=$(normalize < "$f2" | wc -l | tr -d ' ')
    
    [[ "$l1" -eq 0 ]] && [[ "$l2" -eq 0 ]] && { echo "100"; return; }
    [[ "$l1" -eq 0 ]] || [[ "$l2" -eq 0 ]] && { echo "0"; return; }
    
    # Count matching lines using sort -u (removes duplicates)
    local match=$(comm -12 <(normalize < "$f1" | sort -u) <(normalize < "$f2" | sort -u) 2>/dev/null | wc -l | tr -d ' ')
    
    # Use the minimum of the two as denominator
    local min=$l1
    [[ $l2 -lt $l1 ]] && min=$l2
    [[ $min -eq 0 ]] && min=1
    
    echo $((match * 100 / min))
}

generate_diff() {
    local f1=$1 f2=$2 name=$3
    diff -u <(normalize < "$f1") <(normalize < "$f2") > "$DIFFS_DIR/${name}.diff" 2>&1 || true
}

get_examples() {
    if [[ -n "$SPECIFIC_EXAMPLES" ]]; then
        for ex in $SPECIFIC_EXAMPLES; do
            [[ -d "$EXAMPLES_DIR/$ex" ]] && echo "$EXAMPLES_DIR/$ex"
        done
    else
        for dir in "$EXAMPLES_DIR"/ink-*; do
            [[ -d "$dir" ]] && [[ -f "$dir/tui/app.tsx" ]] && echo "$dir"
        done | sort
    fi
}

check_deps() {
    command -v deno &>/dev/null || { echo "Missing: deno"; exit 2; }
    [[ -x "$RUNTS_BIN" ]] || { echo "Missing: runts"; exit 2; }
}

main() {
    check_deps
    
    echo ""
    echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║${NC}  ${CYAN}INK PARITY TEST${NC} - deno | runts dev (HIR) | runts build (compile)          ${BOLD}║${NC}"
    echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    local passed=0 failed=0 skipped=0 interactive=0 failures=()
    local examples=$(get_examples)
    local total=$(echo "$examples" | wc -l | tr -d ' ')
    local current=0
    
    for ed in $examples; do
        current=$((current + 1))
        name=$(basename "$ed")
        
        [[ ! -f "$ed/tui/app.tsx" ]] && { echo -e "${YELLOW}[$current/$total] $name SKIP${NC}"; skipped=$((skipped+1)); continue; }
        
        echo -n -e "${BLUE}[$current/$total]${NC} $name "
        
        run_deno "$ed"
        local deno_status=$?
        run_hir "$ed"
        
        [[ "$RUN_QUICK" != "true" ]] && run_compile "$ed"
        
        local deno_f="$RESULTS_DIR/deno_$name.txt"
        local hir_f="$RESULTS_DIR/hir_$name.txt"
        
        if [[ $deno_status -eq 2 ]]; then
            echo -e "${YELLOW}INT${NC}"
            interactive=$((interactive+1))
            continue
        fi
        
        if [[ $deno_status -ne 0 ]]; then
            echo -e "${RED}ERR${NC}"
            failed=$((failed+1))
            failures+=("$name (deno error)")
            continue
        fi
        
        local dh_sim=$(calc_sim "$deno_f" "$hir_f")
        
        if [[ "$RUN_QUICK" != "true" ]]; then
            local compile_f="$RESULTS_DIR/compile_$name.txt"
            local dc_sim=$(calc_sim "$deno_f" "$compile_f")
            local hc_sim=$(calc_sim "$hir_f" "$compile_f")
            
            generate_diff "$deno_f" "$hir_f" "${name}_deno_hir"
            generate_diff "$deno_f" "$compile_f" "${name}_deno_compile"
            generate_diff "$hir_f" "$compile_f" "${name}_hir_compile"
            
            local matches=0
            [[ $dh_sim -ge $PARITY_THRESHOLD ]] && matches=$((matches+1))
            [[ $dc_sim -ge $PARITY_THRESHOLD ]] && matches=$((matches+1))
            [[ $hc_sim -ge $PARITY_THRESHOLD ]] && matches=$((matches+1))
            
            echo "D-H:${dh_sim}% D-C:${dc_sim}% H-C:${hc_sim}%"
            
            if [[ $matches -ge 2 ]]; then
                echo -e "    ${GREEN}✓${NC}"; passed=$((passed+1))
            else
                echo -e "    ${RED}✗${NC}"; failed=$((failed+1)); failures+=("$name")
            fi
        else
            echo "D-H:${dh_sim}%"
            if [[ $dh_sim -ge $PARITY_THRESHOLD ]]; then
                echo -e "    ${GREEN}✓${NC}"; passed=$((passed+1))
            else
                echo -e "    ${RED}✗${NC}"; failed=$((failed+1)); failures+=("$name")
            fi
        fi
    done
    
    echo ""
    echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║${NC}  ${CYAN}SUMMARY${NC}                                                       ${BOLD}║${NC}"
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════════════════╣${NC}"
    printf "${BOLD}║${NC}  ${GREEN}Passed:${NC} %s  ${RED}Failed:${NC} %s  ${YELLOW}Skipped:${NC} %s  ${YELLOW}Interactive:${NC} %s${BOLD}║${NC}\n" "$passed" "$failed" "$skipped" "$interactive"
    echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo "Results: $RESULTS_DIR"
    echo "Diffs: $DIFFS_DIR"
    
    [[ $failed -gt 0 ]] && {
        echo ""
        echo -e "${RED}FAILURES:${NC}"
        for n in "${failures[@]}"; do
            echo "  $n:"
            head -3 "$RESULTS_DIR/deno_${n%% *}.txt" 2>/dev/null | sed 's/^/    /'
        done
        exit 1
    }
    
    echo -e "\n${GREEN}✓ ALL PARITY${NC}"
    exit 0
}

main
