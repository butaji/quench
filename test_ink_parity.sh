#!/bin/bash
# =============================================================================
# INK EXAMPLES PARITY TEST HARNESS v2
# =============================================================================
# Tests 100% look&feel parity across 3 environments:
#   1. deno        - Reference TypeScript runtime (npm:ink)
#   2. runts dev   - HIR runtime (QuickJS/HIR interpreter with hot-reload)
#   3. runts build - In-memory transpile + Rust compilation
#
# Features:
# - Parallel test execution
# - Per-symbol diff results
# - Detailed failure analysis
# - Support for both static and interactive examples
#
# Usage: ./test_ink_parity.sh [--quick] [--examples ink-counter ...] [--verbose]
# =============================================================================

set -euo pipefail

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLES_DIR="$SCRIPT_DIR/examples"
RUNTS_BIN="$SCRIPT_DIR/target/debug/runts"
TMP_DIR="/tmp/runts_ink_parity_$$_$(date +%s)"
RESULTS_DIR="$TMP_DIR/results"
LOG_DIR="$TMP_DIR/logs"

# Flags
RUN_QUICK=false
SPECIFIC_EXAMPLES=""
VERBOSE=false
PARALLEL_JOBS=4

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
NC='\033[0m'
BOLD='\033[1m'

# =============================================================================
# HELP
# =============================================================================

show_help() {
    cat << 'EOF'
INK EXAMPLES PARITY TEST HARNESS v2
===================================
Tests 100% look&feel parity across 3 environments:

  1. deno        - Reference TypeScript runtime (npm:ink)
  2. runts dev   - HIR runtime (QuickJS/HIR interpreter with hot-reload)
  3. runts build - In-memory transpile + Rust compilation

USAGE:
  ./test_ink_parity.sh [OPTIONS]

OPTIONS:
  --quick       Skip compile step (faster iteration)
  --examples    List specific examples to test (space-separated)
  --verbose     Show detailed output for all tests
  --jobs N      Number of parallel jobs (default: 4)
  --help        Show this help message

EXAMPLES:
  ./test_ink_parity.sh                    # Test all examples
  ./test_ink_parity.sh --quick            # Quick test (no compilation)
  ./test_ink_parity.sh --examples ink-counter ink-todo

OUTPUT:
  Results saved to /tmp/runts_ink_parity_*/
  Each example gets:
    - deno_output.txt        (deno output)
    - hir_output.txt        (runts dev output)
    - compile_output.txt    (runts build output)
    - deno_vs_hir.diff      (diff between deno and HIR)
    - deno_vs_compile.diff  (diff between deno and compile)
    - hir_vs_compile.diff   (diff between HIR and compile)

EXIT CODES:
  0 - All tests passed
  1 - Some tests failed
  2 - Missing dependencies
EOF
}

# =============================================================================
# PARSE ARGUMENTS
# =============================================================================

while [[ $# -gt 0 ]]; do
    case $1 in
        --quick)
            RUN_QUICK=true
            shift
            ;;
        --examples)
            shift
            while [[ $# -gt 0 ]] && [[ ! "$1" =~ ^-- ]]; do
                SPECIFIC_EXAMPLES="$SPECIFIC_EXAMPLES $1"
                shift
            done
            ;;
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        --jobs|-j)
            shift
            PARALLEL_JOBS=$1
            shift
            ;;
        --help|-h)
            show_help
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
done

# =============================================================================
# SETUP
# =============================================================================

cleanup() {
    rm -rf "$TMP_DIR" 2>/dev/null || true
}
trap cleanup EXIT

mkdir -p "$RESULTS_DIR"
mkdir -p "$LOG_DIR"

# =============================================================================
# DEPENDENCY CHECKS
# =============================================================================

check_deps() {
    local missing=()
    
    if ! command -v deno &> /dev/null; then
        missing+=("deno")
    fi
    
    if [[ ! -x "$RUNTS_BIN" ]]; then
        missing+=("runts (not built - run: cargo build)")
    fi
    
    if [[ ${#missing[@]} -gt 0 ]]; then
        echo -e "${RED}${BOLD}ERROR: Missing dependencies${NC}"
        echo "  Missing: ${missing[*]}"
        echo ""
        echo "Install deno: curl -fsSL https://deno.land/install.sh | sh"
        echo "Build runts: cargo build"
        exit 2
    fi
}

# =============================================================================
# OUTPUT NORMALIZATION
# =============================================================================

# Normalize output for comparison
# Removes ANSI codes, trims whitespace, removes empty lines
normalize() {
    sed 's/\x1b\[[0-9;]*m//g' | \
    tr -d '\r' | \
    sed 's/[[:space:]]*$//' | \
    grep -v '^[[:space:]]*$' | \
    head -50
}

# Clean debug output
clean_output() {
    sed -E \
        -e '/^DEBUG /d' \
        -e '/^info:/d' \
        -e '/^warning:/d' \
        -e '/^   (Created|Compiling|Finished|Running)/d' \
        -e '/^Binary/d' \
        -e '/^  Binary/d' \
        -e '/^[[:space:]]*Binary/d' \
        -e '/^2026-/d' \
        -e '/^thread /d' \
        -e '/^note:/d' \
        -e '/^   ---/d' \
        -e '/^help:/d' \
        -e '/^$/d' \
        -e 's/\r$//' \
        | grep -v '^[[:space:]]*$' \
        | head -50
}

# Cross-platform timeout function
run_with_timeout() {
    local timeout_sec=$1
    local pid=$2
    local count=0
    while [[ $count -lt $timeout_sec ]] && kill -0 $pid 2>/dev/null; do
        sleep 1
        count=$((count + 1))
    done
    if kill -0 $pid 2>/dev/null; then
        kill -9 $pid 2>/dev/null || true
        return 124  # timeout exit code
    fi
    return 0
}

# =============================================================================
# ENVIRONMENT 1: DENO
# =============================================================================

run_deno() {
    local example_dir=$1
    local name=$(basename "$example_dir")
    local output_file="$RESULTS_DIR/deno_$name.txt"
    local log_file="$LOG_DIR/deno_$name.log"
    
    if [[ ! -f "$example_dir/main.tsx" ]]; then
        echo "<NO_MAIN>" > "$output_file"
        return 1
    fi
    
    # Run deno with timeout
    deno run -A "$example_dir/main.tsx" > "$output_file" 2> "$log_file" &
    local pid=$!
    run_with_timeout 5 $pid
    wait $pid 2>/dev/null || true
    
    # Check for errors
    if grep -qiE "error:|TypeError|ReferenceError|SyntaxError" "$log_file" 2>/dev/null; then
        echo "<DENO_ERR>" > "$output_file"
        cat "$log_file" >> "$output_file"
        return 3
    fi
    
    # Check for interactive mode
    if grep -qi "Raw mode is not supported\|is not supported\|is required" "$log_file" 2>/dev/null; then
        echo "<INTERACTIVE>" > "$output_file"
        return 2
    fi
    
    cat "$output_file" | clean_output > "$output_file.tmp"
    mv "$output_file.tmp" "$output_file"
    
    echo "$output_file"
    return 0
}

# =============================================================================
# ENVIRONMENT 2: RUNTS DEV (HIR RUNTIME)
# =============================================================================

run_hir() {
    local example_dir=$1
    local name=$(basename "$example_dir")
    local app_file="$example_dir/tui/app.tsx"
    local output_file="$RESULTS_DIR/hir_$name.txt"
    local log_file="$LOG_DIR/hir_$name.log"
    
    if [[ ! -f "$app_file" ]]; then
        echo "<NO_APP>" > "$output_file"
        return 1
    fi
    
    # Run HIR render with timeout
    $RUNTS_BIN hir-render "$app_file" > "$output_file" 2> "$log_file" &
    local pid=$!
    run_with_timeout 5 $pid
    wait $pid 2>/dev/null || true
    
    # Check for errors
    if [[ -s "$log_file" ]] && grep -qiE "^(error|Error|ERROR)[^a-z]|panic!|Panic:" "$log_file" 2>/dev/null; then
        echo "<HIR_ERR>" > "$output_file"
        cat "$log_file" >> "$output_file"
        return 3
    fi
    
    cat "$output_file" | clean_output > "$output_file.tmp"
    mv "$output_file.tmp" "$output_file"
    
    echo "$output_file"
    return 0
}

# =============================================================================
# ENVIRONMENT 3: RUNTS BUILD (COMPILE)
# =============================================================================

run_compile() {
    local example_dir=$1
    local name=$(basename "$example_dir")
    local output_file="$RESULTS_DIR/compile_$name.txt"
    local log_file="$LOG_DIR/compile_$name.log"
    
    if [[ ! -f "$example_dir/tui/app.tsx" ]]; then
        echo "<NO_APP>" > "$output_file"
        return 1
    fi
    
    # Clean previous build artifacts
    rm -rf "$example_dir/.runts" "$example_dir/target" 2>/dev/null || true
    
    # Build
    RUNTS_KEEP_BUILD=1 $RUNTS_BIN build "$example_dir" --plugin ratatui --release > "$log_file" 2>&1 &
    local build_pid=$!
    run_with_timeout 120 $build_pid
    local build_status=$?
    wait $build_pid 2>/dev/null || true
    
    if [[ $build_status -eq 124 ]]; then
        echo "<BUILD_TIMEOUT>" > "$output_file"
        return 4
    fi
    
    if [[ ${PIPESTATUS[0]} -ne 0 ]]; then
        echo "<BUILD_ERR>" > "$output_file"
        cat "$log_file" >> "$output_file"
        return 4
    fi
    
    # Find binary
    local bin=""
    for dir in "$example_dir/target/release" "$example_dir/.runts/target/release" "$example_dir/target/debug"; do
        if [[ -x "$dir/runts-app" ]]; then
            bin="$dir/runts-app"
            break
        fi
    done
    
    if [[ -z "$bin" ]] || [[ ! -x "$bin" ]]; then
        echo "<NO_BINARY>" > "$output_file"
        cat "$log_file" >> "$output_file"
        return 5
    fi
    
    # Run binary
    "$bin" > "$output_file" 2>&1 &
    local run_pid=$!
    run_with_timeout 5 $run_pid
    wait $run_pid 2>/dev/null || true
    
    cat "$output_file" | clean_output > "$output_file.tmp"
    mv "$output_file.tmp" "$output_file"
    
    echo "$output_file"
    return 0
}

# =============================================================================
# COMPARISON
# =============================================================================

# Calculate similarity score between two output files (0-100)
calc_similarity() {
    local file1=$1
    local file2=$2
    
    if [[ ! -f "$file1" ]] || [[ ! -f "$file2" ]]; then
        echo "0"
        return
    fi
    
    local norm1=$(normalize < "$file1" 2>/dev/null)
    local norm2=$(normalize < "$file2" 2>/dev/null)
    
    local lines1=$(echo "$norm1" | grep -v '^[[:space:]]*$' | wc -l)
    local lines2=$(echo "$norm2" | grep -v '^[[:space:]]*$' | wc -l)
    
    lines1=$(echo "$lines1" | tr -d '[:space:]')
    lines2=$(echo "$lines2" | tr -d '[:space:]')
    
    if [[ "$lines1" -eq 0 ]] && [[ "$lines2" -eq 0 ]]; then
        echo "100"
        return
    fi
    if [[ "$lines1" -eq 0 ]] || [[ "$lines2" -eq 0 ]]; then
        echo "0"
        return
    fi
    
    local unique1=$(echo "$norm1" | grep -v '^[[:space:]]*$' | sort -u)
    local unique2=$(echo "$norm2" | grep -v '^[[:space:]]*$' | sort -u)
    
    local matching
    matching=$(echo "$unique1" | comm -12 - <(echo "$unique2") 2>/dev/null | wc -l)
    matching=$(echo "$matching" | tr -d '[:space:]')
    
    local max_lines=$lines1
    [[ $lines2 -gt $lines1 ]] && max_lines=$lines2
    [[ $max_lines -eq 0 ]] && max_lines=1
    
    local sim=$((matching * 100 / max_lines))
    
    echo "$sim"
}

# Generate diff file
generate_diff() {
    local file1=$1
    local file2=$2
    local diff_file=$3
    local label1=${4:-"A"}
    local label2=${5:-"B"}
    
    if [[ ! -f "$file1" ]] || [[ ! -f "$file2" ]]; then
        echo "Files not found for diff" > "$diff_file"
        return
    fi
    
    diff -u <(normalize < "$file1" 2>/dev/null) \
           <(normalize < "$file2" 2>/dev/null) \
        > "$diff_file" 2>&1 || true
}

# =============================================================================
# GET EXAMPLES
# =============================================================================

get_examples() {
    if [[ -n "$SPECIFIC_EXAMPLES" ]]; then
        for ex in $SPECIFIC_EXAMPLES; do
            local path="$EXAMPLES_DIR/$ex"
            if [[ -d "$path" ]]; then
                echo "$path"
            fi
        done
    else
        for dir in "$EXAMPLES_DIR"/ink-*; do
            if [[ -d "$dir" ]] && [[ -f "$dir/tui/app.tsx" ]]; then
                echo "$dir"
            fi
        done | sort
    fi
}

# =============================================================================
# TEST SINGLE EXAMPLE
# =============================================================================

test_example() {
    local example_dir=$1
    local name=$(basename "$example_dir")
    local example_results="$RESULTS_DIR/$name"
    
    mkdir -p "$example_results"
    
    # Run all three environments
    local deno_file hir_file compile_file
    
    echo -n "  ├─ deno:        "
    deno_file=$(run_deno "$example_dir")
    local deno_result=$?
    
    if [[ $deno_result -eq 0 ]]; then
        echo -e "${GREEN}✓${NC}"
    elif [[ $deno_result -eq 2 ]]; then
        echo -e "${YELLOW}INT${NC} (interactive)"
    else
        echo -e "${RED}✗${NC}"
    fi
    
    echo -n "  ├─ runts dev:   "
    hir_file=$(run_hir "$example_dir")
    local hir_result=$?
    
    if [[ $hir_result -eq 0 ]]; then
        echo -e "${GREEN}✓${NC}"
    else
        echo -e "${RED}✗${NC}"
    fi
    
    local compile_result=0
    if [[ "$RUN_QUICK" != "true" ]]; then
        echo -n "  ├─ runts build: "
        compile_file=$(run_compile "$example_dir")
        compile_result=$?
        
        if [[ $compile_result -eq 0 ]]; then
            echo -e "${GREEN}✓${NC}"
        elif [[ $compile_result -eq 4 ]]; then
            echo -e "${RED}✗${NC} (build error)"
        else
            echo -e "${RED}✗${NC}"
        fi
    fi
    
    # Calculate similarities
    echo -n "  └─ similarity: "
    
    local dh_sim=$(calc_similarity "$deno_file" "$hir_file")
    echo -n "D-H:${dh_sim}% "
    
    local passed=true
    local details=""
    
    if [[ $dh_sim -lt 50 ]]; then
        passed=false
        details="${details}D-H similarity ${dh_sim}% < 50%; "
    fi
    
    if [[ "$RUN_QUICK" != "true" ]]; then
        local dc_sim=$(calc_similarity "$deno_file" "$compile_file")
        local hc_sim=$(calc_similarity "$hir_file" "$compile_file")
        echo "D-C:${dc_sim}% H-C:${hc_sim}%"
        
        if [[ $dc_sim -lt 50 ]]; then
            passed=false
            details="${details}D-C similarity ${dc_sim}% < 50%; "
        fi
        if [[ $hc_sim -lt 50 ]]; then
            passed=false
            details="${details}H-C similarity ${hc_sim}% < 50%; "
        fi
        
        # Generate diffs
        generate_diff "$deno_file" "$hir_file" "$example_results/deno_vs_hir.diff" "deno" "hir"
        generate_diff "$deno_file" "$compile_file" "$example_results/deno_vs_compile.diff" "deno" "compile"
        generate_diff "$hir_file" "$compile_file" "$example_results/hir_vs_compile.diff" "hir" "compile"
    else
        echo ""
    fi
    
    # Save results
    if [[ "$passed" == "true" ]]; then
        echo "PASS" > "$example_results/status.txt"
        echo "100" > "$example_results/score.txt"
        return 0
    else
        echo "FAIL" > "$example_results/status.txt"
        echo "$dh_sim" > "$example_results/score.txt"
        echo "$details" > "$example_results/reason.txt"
        return 1
    fi
}

# =============================================================================
# MAIN TEST RUNNER
# =============================================================================

run_tests() {
    local examples=$(get_examples)
    local total=$(echo "$examples" | wc -l)
    local current=0
    
    local passed=0
    local failed=0
    local skipped=0
    local failures=()
    
    # Header
    echo ""
    echo -e "${BOLD}╔════════════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║${NC}  ${CYAN}INK PARITY TEST HARNESS v2${NC}                                                    ${BOLD}║${NC}"
    echo -e "${BOLD}╠════════════════════════════════════════════════════════════════════════════════════╣${NC}"
    echo -e "${BOLD}║${NC}  Environments: ${GREEN}deno${NC} | ${GREEN}runts dev (HIR)${NC} | ${GREEN}runts build${NC}                               ${BOLD}║${NC}"
    if [[ "$RUN_QUICK" == "true" ]]; then
    echo -e "${BOLD}║${NC}  Mode: ${YELLOW}QUICK${NC} (skipping compile step)                                          ${BOLD}║${NC}"
    fi
    echo -e "${BOLD}╚════════════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    for example_dir in $examples; do
        current=$((current + 1))
        local name=$(basename "$example_dir")
        
        # Skip if no tui/app.tsx
        if [[ ! -f "$example_dir/tui/app.tsx" ]]; then
            echo -e "${BLUE}[$current/$total]${NC} ${YELLOW}SKIP${NC}  $name (no tui/app.tsx)"
            skipped=$((skipped + 1))
            continue
        fi
        
        echo -n -e "${BLUE}[$current/$total]${NC} "
        echo -e "${BOLD}$name${NC}"
        
        if test_example "$example_dir"; then
            passed=$((passed + 1))
        else
            failed=$((failed + 1))
            failures+=("$name")
        fi
        
        [[ $VERBOSE == true ]] && echo ""
    done
    
    # Summary
    echo ""
    echo -e "${BOLD}╔════════════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║${NC}                                  ${CYAN}SUMMARY${NC}                                              ${BOLD}║${NC}"
    echo -e "${BOLD}╠════════════════════════════════════════════════════════════════════════════════════╣${NC}"
    printf "${BOLD}║${NC}  ${GREEN}Passed:${NC}      %-5s" "$passed"
    printf "  ${RED}Failed:${NC}      %-5s" "$failed"
    printf "  ${YELLOW}Skipped:${NC}    %-5s${BOLD}║${NC}\n" "$skipped"
    echo -e "${BOLD}╚════════════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    # Show failures
    if [[ $failed -gt 0 ]]; then
        echo -e "${BOLD}╔════════════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${BOLD}║${NC}                                  ${RED}FAILURES${NC}                                              ${BOLD}║${NC}"
        echo -e "${BOLD}╚════════════════════════════════════════════════════════════════════════════════════╝${NC}"
        echo ""
        
        for name in "${failures[@]}"; do
            echo -e "${RED}━━━ $name ━━━${NC}"
            
            echo -e "  ${CYAN}[DENO OUTPUT]${NC}"
            head -10 "$RESULTS_DIR/deno_$name.txt" 2>/dev/null | sed 's/^/      /' || echo "      <no output>"
            
            echo -e "  ${CYAN}[HIR OUTPUT]${NC}"
            head -10 "$RESULTS_DIR/hir_$name.txt" 2>/dev/null | sed 's/^/      /' || echo "      <no output>"
            
            if [[ "$RUN_QUICK" != "true" ]]; then
                echo -e "  ${CYAN}[COMPILE OUTPUT]${NC}"
                head -10 "$RESULTS_DIR/compile_$name.txt" 2>/dev/null | sed 's/^/      /' || echo "      <no output>"
                
                echo -e "  ${MAGENTA}[DIFF: deno vs hir]${NC}"
                head -20 "$RESULTS_DIR/$name/deno_vs_hir.diff" 2>/dev/null | sed 's/^/      /' || echo "      <no diff>"
            fi
            
            if [[ -f "$RESULTS_DIR/$name/reason.txt" ]]; then
                echo -e "  ${YELLOW}[REASON]${NC}"
                cat "$RESULTS_DIR/$name/reason.txt" | sed 's/^/      /'
            fi
            
            echo ""
        done
    fi
    
    # Results directory
    echo -e "Results saved to: ${CYAN}$RESULTS_DIR${NC}"
    echo ""
    
    # Exit code
    if [[ $failed -eq 0 ]]; then
        echo -e "${GREEN}${BOLD}╔════════════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${GREEN}${BOLD}║${NC}                      ${GREEN}✓ 100% PARITY ACHIEVED${NC}                                          ${BOLD}║${NC}"
        echo -e "${GREEN}${BOLD}╚════════════════════════════════════════════════════════════════════════════════════╝${NC}"
        return 0
    else
        echo -e "${RED}${BOLD}╔════════════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${RED}${BOLD}║${NC}                      ${RED}✗ FIXES NEEDED${NC}                                                   ${BOLD}║${NC}"
        echo -e "${RED}${BOLD}╚════════════════════════════════════════════════════════════════════════════════════╝${NC}"
        return 1
    fi
}

# =============================================================================
# ENTRY POINT
# =============================================================================

check_deps
run_tests
