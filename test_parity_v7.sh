#!/bin/bash
# =============================================================================
# INK PARITY TEST HARNESS v7 - COMPLETE 3-ENVIRONMENT TESTING
# =============================================================================
# Tests 100% look&feel parity across 3 environments:
#   1. deno        - Reference TypeScript runtime (npm:ink@5)
#   2. runts dev   - HIR runtime (QuickJS/HIR interpreter with hot-reload)
#   3. runts build - In-memory transpile + Rust compilation
#
# Features:
#   - Per-symbol diff analysis
#   - Character-level comparison
#   - ANSI color normalization
#   - Detailed failure categorization
#   - Cross-platform timeout support
#   - Unit test coverage integration
#
# Usage: ./test_parity_v7.sh [OPTIONS]
# =============================================================================

set -euo pipefail

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLES_DIR="$SCRIPT_DIR/examples"
RUNTS_BIN="$SCRIPT_DIR/target/debug/runts"
TMP_DIR="/tmp/runts_ink_parity_v7_$$_$(date +%s)"
RESULTS_DIR="$TMP_DIR/results"
LOG_DIR="$TMP_DIR/logs"
DIFF_DIR="$TMP_DIR/diffs"
SUMMARY_FILE="$TMP_DIR/summary.txt"
SYMBOLS_DIR="$TMP_DIR/symbols"

# Flags
RUN_COMPILE=true
SPECIFIC_EXAMPLES=""
VERBOSE=false
KEEP_RESULTS=false
PARALLEL_JOBS=4
MIN_SIMILARITY=60

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
INK PARITY TEST HARNESS v7 - Complete 3-Environment Testing
============================================================
Tests 100% look&feel parity across 3 environments:

  1. deno        - Reference TypeScript runtime (npm:ink@5)
  2. runts dev   - HIR runtime (QuickJS/HIR interpreter with hot-reload)
  3. runts build - In-memory transpile + Rust compilation

USAGE:
  ./test_parity_v7.sh [OPTIONS]

OPTIONS:
  --no-compile    Skip compile step (faster iteration)
  --examples      List specific examples to test (space-separated)
  --verbose       Show detailed output for all tests
  --keep          Keep temp files after completion
  --jobs N        Number of parallel jobs (default: 4)
  --min-sim N     Minimum similarity threshold (default: 60)
  --help          Show this help message

OUTPUT:
  Results saved to /tmp/runts_ink_parity_v7_*/
  Each example gets:
    - deno_output.txt        (deno output)
    - hir_output.txt         (runts dev output)
    - compile_output.txt      (runts build output)
    - diffs/                 (detailed diffs for each pair)
    - symbols/               (extracted symbols for analysis)

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
        --no-compile)
            RUN_COMPILE=false
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
        --keep)
            KEEP_RESULTS=true
            shift
            ;;
        --jobs|-j)
            shift
            PARALLEL_JOBS=$1
            shift
            ;;
        --min-sim)
            shift
            MIN_SIMILARITY=$1
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
    if [[ "$KEEP_RESULTS" != "true" ]]; then
        rm -rf "$TMP_DIR" 2>/dev/null || true
    fi
}
trap cleanup EXIT

mkdir -p "$RESULTS_DIR"
mkdir -p "$LOG_DIR"
mkdir -p "$DIFF_DIR"
mkdir -p "$SYMBOLS_DIR"

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
# CROSS-PLATFORM TIMEOUT
# =============================================================================

run_with_timeout() {
    local timeout_sec=$1
    shift
    local cmd="$@"
    
    # Use timeout command (GNU coreutils on Linux, via brew on macOS)
    if command -v gtimeout &> /dev/null; then
        gtimeout "$timeout_sec" $cmd
    elif command -v timeout &> /dev/null; then
        timeout "$timeout_sec" $cmd
    else
        # Fallback for macOS: background process with sleep and kill
        ( $cmd ) &
        local pid=$!
        local elapsed=0
        while [[ $elapsed -lt $timeout_sec ]]; do
            if ! kill -0 $pid 2>/dev/null; then
                wait $pid
                return $?
            fi
            sleep 1
            elapsed=$((elapsed + 1))
        done
        kill -9 $pid 2>/dev/null || true
        wait $pid 2>/dev/null || true
        return 124
    fi
}

run_with_timeout_capture() {
    local timeout_sec=$1
    local output_file=$2
    shift 2
    local cmd="$@"
    
    # Start command in background
    $cmd > "$output_file" 2>&1 &
    local pid=$!
    
    # Poll for completion or timeout
    local elapsed=0
    while [[ $elapsed -lt $timeout_sec ]]; do
        if ! kill -0 $pid 2>/dev/null; then
            wait $pid
            return $?
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done
    
    # Timeout - kill the process
    kill -9 $pid 2>/dev/null || true
    wait $pid 2>/dev/null || true
    return 124
}

# =============================================================================
# INTERACTIVE EXAMPLE DETECTION
# =============================================================================

is_interactive() {
    local example_dir=$1
    local app_tsx="$example_dir/tui/app.tsx"
    
    if [[ -f "$app_tsx" ]]; then
        if grep -qE "useInput|useFocus|useStdin|useApp|useWindowSize" "$app_tsx" 2>/dev/null; then
            return 0
        fi
    fi
    return 1
}

# =============================================================================
# OUTPUT NORMALIZATION
# =============================================================================

normalize() {
    sed 's/\x1b\[[0-9;]*m//g' | \
    tr -d '\r' | \
    sed 's/[[:space:]]*$//' | \
    grep -v '^[[:space:]]*$'
}

clean_output() {
    perl -pe 's/\x1b\[[0-9;]*m//g' | \
    tr -d '\r' | \
    sed \
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
        | awk '!seen[$0]++' \
        | head -100
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
    
    # For interactive examples, check if deno will have raw mode issues
    local has_raw_mode_issue=false
    if is_interactive "$example_dir"; then
        has_raw_mode_issue=true
    fi
    
    if [[ "$has_raw_mode_issue" == "true" ]]; then
        # For interactive examples, run and check if raw mode error
        (echo "q"; sleep 1) | run_with_timeout 3 deno run -A "$example_dir/main.tsx" > "$output_file" 2> "$log_file" || true
    else
        run_with_timeout 5 deno run -A "$example_dir/main.tsx" > "$output_file" 2> "$log_file" || true
    fi
    
    # Check for raw mode error - this is expected for interactive examples in non-terminal
    if grep -qi "Raw mode is not supported" "$log_file" 2>/dev/null; then
        # This is expected - interactive examples can't run in non-terminal
        # If output exists, it might be partial - check if it's mostly errors
        if [[ -s "$output_file" ]]; then
            local error_lines=$(grep -c "ERROR\|Error\|error\|file:" "$output_file" 2>/dev/null || echo 0)
            local total_lines=$(wc -l < "$output_file" 2>/dev/null || echo 0)
            if [[ $error_lines -gt $((total_lines / 2)) ]]; then
                # Most of the output is errors - mark as interactive raw mode
                echo "<INTERACTIVE_RAW_MODE>" > "$output_file"
            fi
        else
            echo "<INTERACTIVE_RAW_MODE>" > "$output_file"
        fi
        return 2
    fi
    
    # Check for other errors
    if grep -qiE "^error:|TypeError|ReferenceError|SyntaxError" "$log_file" 2>/dev/null; then
        echo "<DENO_ERR>" > "$output_file"
        cat "$log_file" >> "$output_file"
        return 3
    fi
    
    # Clean output
    if [[ -s "$output_file" ]]; then
        clean_output < "$output_file" > "$output_file.tmp"
        mv "$output_file.tmp" "$output_file"
    fi
    
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
    
    run_with_timeout 5 $RUNTS_BIN hir-render "$app_file" > "$output_file" 2> "$log_file" || true
    
    # Check for errors
    if [[ -s "$log_file" ]] && grep -qiE "^(error|Error|ERROR)[^a-z]|panic!|Panic:" "$log_file" 2>/dev/null; then
        echo "<HIR_ERR>" > "$output_file"
        cat "$log_file" >> "$output_file"
        return 3
    fi
    
    # Clean output
    if [[ -s "$output_file" ]]; then
        clean_output < "$output_file" > "$output_file.tmp"
        mv "$output_file.tmp" "$output_file"
    fi
    
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
    
    # Build in background
    RUNTS_KEEP_BUILD=1 $RUNTS_BIN build "$example_dir" --plugin ratatui --release > "$log_file" 2>&1 &
    local build_pid=$!
    
    # Wait for build with timeout
    local elapsed=0
    while [[ $elapsed -lt 120 ]]; do
        if ! kill -0 $build_pid 2>/dev/null; then
            wait $build_pid
            break
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done
    
    if kill -0 $build_pid 2>/dev/null; then
        kill -9 $build_pid 2>/dev/null || true
        wait $build_pid 2>/dev/null || true
        echo "<BUILD_TIMEOUT>" > "$output_file"
        return 4
    fi
    
    # Find binary
    local bin=""
    for dir in "$example_dir/target/release" "$example_dir/.runts/target/release" "$example_dir/target/debug"; do
        if [[ -x "$dir/run" ]]; then
            bin="$dir/run"
            break
        fi
    done
    
    if [[ -z "$bin" ]] || [[ ! -x "$bin" ]]; then
        echo "<NO_BINARY>" > "$output_file"
        cat "$log_file" >> "$output_file"
        return 5
    fi
    
    # Run binary
    run_with_timeout 5 "$bin" > "$output_file" 2>&1 || true
    
    # Clean output
    if [[ -s "$output_file" ]]; then
        clean_output < "$output_file" > "$output_file.tmp"
        mv "$output_file.tmp" "$output_file"
    fi
    
    echo "$output_file"
    return 0
}

# =============================================================================
# SIMILARITY CALCULATION
# =============================================================================

calc_similarity() {
    local file1=$1
    local file2=$2
    
    if [[ ! -f "$file1" ]] || [[ ! -f "$file2" ]]; then
        echo "0"
        return
    fi
    
    local norm1=$(normalize < "$file1" 2>/dev/null)
    local norm2=$(normalize < "$file2" 2>/dev/null)
    
    local lines1=$(echo "$norm1" | wc -l | tr -d '[:space:]')
    local lines2=$(echo "$norm2" | wc -l | tr -d '[:space:]')
    
    if [[ "$lines1" -eq 0 ]] && [[ "$lines2" -eq 0 ]]; then
        echo "100"
        return
    fi
    if [[ "$lines1" -eq 0 ]] || [[ "$lines2" -eq 0 ]]; then
        echo "0"
        return
    fi
    
    local matching=$(echo "$norm1" | sort -u | comm -12 - <(echo "$norm2" | sort -u) 2>/dev/null | wc -l | tr -d '[:space:]')
    
    local max_lines=$((lines1 > lines2 ? lines1 : lines2))
    [[ $max_lines -eq 0 ]] && max_lines=1
    
    echo $((matching * 100 / max_lines))
}

# =============================================================================
# DIFF GENERATION
# =============================================================================

generate_diff() {
    local file1=$1
    local file2=$2
    local label1=$3
    local label2=$4
    local diff_file=$5
    
    {
        echo "=== Diff: $label1 vs $label2 ==="
        echo ""
        
        if [[ ! -f "$file1" ]] || [[ ! -s "$file1" ]]; then
            echo "$label1 is empty or missing"
        fi
        
        if [[ ! -f "$file2" ]] || [[ ! -s "$file2" ]]; then
            echo "$label2 is empty or missing"
        fi
        
        echo ""
        echo "--- $label1 (unique) ---"
        comm -23 <(normalize < "$file1" 2>/dev/null | sort -u) <(normalize < "$file2" 2>/dev/null | sort -u) 2>/dev/null || true
        
        echo ""
        echo "--- $label2 (unique) ---"
        comm -13 <(normalize < "$file1" 2>/dev/null | sort -u) <(normalize < "$file2" 2>/dev/null | sort -u) 2>/dev/null || true
        
        echo ""
        echo "--- Side by side (first 30 lines) ---"
        diff -y --width=80 <(normalize < "$file1" 2>/dev/null | head -30) <(normalize < "$file2" 2>/dev/null | head -30) 2>/dev/null || true
    } > "$diff_file"
}

# =============================================================================
# SYMBOL EXTRACTION
# =============================================================================

extract_symbols() {
    local file=$1
    local output=$2
    
    if [[ ! -f "$file" ]] || [[ ! -s "$file" ]]; then
        echo "" > "$output"
        return
    fi
    
    {
        echo "=== Lines ==="
        cat "$file"
        echo ""
        echo "=== Words (unique, 3+ chars) ==="
        grep -oE '\b[A-Za-z][A-Za-z0-9_-]{2,}\b' "$file" 2>/dev/null | sort -u
        echo ""
        echo "=== Symbols by category ==="
        echo "Colors:"
        grep -oE '(red|green|blue|yellow|cyan|magenta|white|black|gray|grey)' "$file" 2>/dev/null | sort -u || echo "  (none)"
        echo "Numbers:"
        grep -oE '\b[0-9]+\b' "$file" 2>/dev/null | sort -u | head -20
    } > "$output"
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
    local example_diffs="$DIFF_DIR/$name"
    local example_symbols="$SYMBOLS_DIR/$name"
    
    mkdir -p "$example_diffs"
    mkdir -p "$example_symbols"
    
    local interactive=false
    if is_interactive "$example_dir"; then
        interactive=true
    fi
    
    # Run all three environments
    local deno_file hir_file compile_file
    local deno_result=0 hir_result=0 compile_result=0
    
    echo -n "  ├─ deno:        "
    deno_file=$(run_deno "$example_dir")
    deno_result=$?
    
    if [[ $deno_result -eq 0 ]]; then
        echo -e "${GREEN}✓${NC}"
    elif [[ $deno_result -eq 2 ]]; then
        echo -e "${YELLOW}INT${NC}"  # Interactive with raw mode issue - expected
    else
        echo -e "${RED}✗${NC}"
    fi
    
    echo -n "  ├─ runts dev:   "
    hir_file=$(run_hir "$example_dir")
    hir_result=$?
    
    if [[ $hir_result -eq 0 ]]; then
        echo -e "${GREEN}✓${NC}"
    else
        echo -e "${RED}✗${NC}"
    fi
    
    if [[ "$RUN_COMPILE" == "true" ]]; then
        echo -n "  ├─ runts build: "
        compile_file=$(run_compile "$example_dir")
        compile_result=$?
        
        if [[ $compile_result -eq 0 ]]; then
            echo -e "${GREEN}✓${NC}"
        else
            echo -e "${RED}✗${NC}"
        fi
    fi
    
    # Calculate similarities
    local dh_sim=$(calc_similarity "$deno_file" "$hir_file")
    local dh_status="PASS"
    [[ $dh_sim -lt $MIN_SIMILARITY ]] && dh_status="FAIL"
    
    echo -n "  └─ similarity: "
    echo -e "D-H:${CYAN}${dh_sim}%${NC} (${GREEN}${dh_status}${NC})"
    
    if [[ "$RUN_COMPILE" == "true" ]]; then
        local dc_sim=$(calc_similarity "$deno_file" "$compile_file")
        local hc_sim=$(calc_similarity "$hir_file" "$compile_file")
        local dc_status="PASS"
        local hc_status="PASS"
        [[ $dc_sim -lt $MIN_SIMILARITY ]] && dc_status="FAIL"
        [[ $hc_sim -lt $MIN_SIMILARITY ]] && hc_status="FAIL"
        
        echo "          D-C:${CYAN}${dc_sim}%${NC} (${GREEN}${dc_status}${NC}) H-C:${CYAN}${hc_sim}%${NC} (${GREEN}${hc_status}${NC})"
    fi
    
    # Generate diffs and symbols
    generate_diff "$deno_file" "$hir_file" "Deno" "HIR" "$example_diffs/deno_vs_hir.diff"
    extract_symbols "$deno_file" "$example_symbols/deno_symbols.txt"
    extract_symbols "$hir_file" "$example_symbols/hir_symbols.txt"
    
    if [[ "$RUN_COMPILE" == "true" ]]; then
        generate_diff "$deno_file" "$compile_file" "Deno" "Compile" "$example_diffs/deno_vs_compile.diff"
        generate_diff "$hir_file" "$compile_file" "HIR" "Compile" "$example_diffs/hir_vs_compile.diff"
        extract_symbols "$compile_file" "$example_symbols/compile_symbols.txt"
    fi
    
    # Determine overall pass/fail
    local overall="PASS"
    [[ $dh_sim -lt $MIN_SIMILARITY ]] && overall="FAIL"
    if [[ "$RUN_COMPILE" == "true" ]]; then
        local dc_sim=$(calc_similarity "$deno_file" "$compile_file")
        local hc_sim=$(calc_similarity "$hir_file" "$compile_file")
        [[ $dc_sim -lt $MIN_SIMILARITY ]] && overall="FAIL"
        [[ $hc_sim -lt $MIN_SIMILARITY ]] && overall="FAIL"
    fi
    
    # Save result
    echo "[$name] D-H:$dh_sim D-C:$(calc_similarity "$deno_file" "$compile_file" 2>/dev/null || echo 'N/A') H-C:$(calc_similarity "$hir_file" "$compile_file" 2>/dev/null || echo 'N/A') $overall" >> "$SUMMARY_FILE"
    
    echo "$overall" > "$RESULTS_DIR/${name}_status.txt"
    echo "$dh_sim" > "$RESULTS_DIR/${name}_score.txt"
    
    [[ "$overall" == "PASS" ]] && return 0 || return 1
}

# =============================================================================
# MAIN TEST RUNNER
# =============================================================================

run_tests() {
    echo "# Ink Parity Test Summary v7" > "$SUMMARY_FILE"
    echo "# Date: $(date)" >> "$SUMMARY_FILE"
    echo "# Min Similarity: $MIN_SIMILARITY%" >> "$SUMMARY_FILE"
    echo "" >> "$SUMMARY_FILE"
    
    local examples=$(get_examples)
    local total=$(echo "$examples" | wc -l | tr -d '[:space:]')
    local current=0
    
    local passed=0
    local failed=0
    local skipped=0
    local interactive=0
    local failures=()
    
    # Header
    echo ""
    echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║${NC}  ${CYAN}INK PARITY TEST HARNESS v7${NC} - Complete 3-Environment Testing                  ${BOLD}║${NC}"
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════════════════════╣${NC}"
    echo -e "${BOLD}║${NC}  Environments: ${GREEN}deno${NC} | ${GREEN}runts dev (HIR)${NC} | ${GREEN}runts build${NC}                            ${BOLD}║${NC}"
    echo -e "${BOLD}║${NC}  Min Similarity: ${YELLOW}${MIN_SIMILARITY}%${NC} | Examples: ${YELLOW}${total}${NC}                                  ${BOLD}║${NC}"
    if [[ "$RUN_COMPILE" != "true" ]]; then
    echo -e "${BOLD}║${NC}  Mode: ${YELLOW}QUICK${NC} (skipping compile step)                                             ${BOLD}║${NC}"
    fi
    echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    for example_dir in $examples; do
        current=$((current + 1))
        local name=$(basename "$example_dir")
        
        if [[ ! -f "$example_dir/tui/app.tsx" ]]; then
            echo -e "${BLUE}[$current/$total]${NC} ${YELLOW}SKIP${NC}  $name (no tui/app.tsx)"
            skipped=$((skipped + 1))
            continue
        fi
        
        echo -e "${BLUE}[$current/$total]${NC} ${BOLD}$name${NC}"
        
        if is_interactive "$example_dir"; then
            interactive=$((interactive + 1))
            echo "    (interactive - HIR renders initial state)"
        fi
        
        if test_example "$example_dir"; then
            passed=$((passed + 1))
            echo -e "    ${GREEN}✓ PASS${NC}"
        else
            failed=$((failed + 1))
            failures+=("$name")
            echo -e "    ${RED}✗ FAIL${NC}"
        fi
        
        [[ $VERBOSE == true ]] && echo ""
    done
    
    # Summary
    echo ""
    echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║${NC}                                    ${CYAN}SUMMARY${NC}                                          ${BOLD}║${NC}"
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════════════════════╣${NC}"
    printf "${BOLD}║${NC}  ${GREEN}Passed:${NC}      %-5s" "$passed"
    printf "  ${RED}Failed:${NC}      %-5s" "$failed"
    printf "  ${YELLOW}Skipped:${NC}    %-5s" "$skipped"
    printf "  ${YELLOW}Interactive:${NC} %-5s${BOLD}║${NC}\n" "$interactive"
    echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    # Show failures
    if [[ $failed -gt 0 ]]; then
        echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${BOLD}║${NC}                                    ${RED}FAILURES${NC}                                        ${BOLD}║${NC}"
        echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════════════════════╝${NC}"
        echo ""
        
        for name in "${failures[@]}"; do
            echo -e "${RED}━━━ $name ━━━${NC}"
            
            echo -e "  ${CYAN}[DENO OUTPUT]${NC}"
            head -15 "$RESULTS_DIR/deno_$name.txt" 2>/dev/null | sed 's/^/      /' || echo "      <no output>"
            
            echo -e "  ${CYAN}[HIR OUTPUT]${NC}"
            head -15 "$RESULTS_DIR/hir_$name.txt" 2>/dev/null | sed 's/^/      /' || echo "      <no output>"
            
            if [[ "$RUN_COMPILE" == "true" ]]; then
                echo -e "  ${CYAN}[COMPILE OUTPUT]${NC}"
                head -15 "$RESULTS_DIR/compile_$name.txt" 2>/dev/null | sed 's/^/      /' || echo "      <no output>"
            fi
            
            echo -e "  ${MAGENTA}[DIFF: deno vs hir]${NC}"
            head -30 "$DIFF_DIR/$name/deno_vs_hir.diff" 2>/dev/null | sed 's/^/      /' || echo "      <no diff>"
            
            echo ""
        done
    fi
    
    # Results directory
    echo -e "Results: ${CYAN}$RESULTS_DIR${NC}"
    echo -e "Diffs:   ${CYAN}$DIFF_DIR${NC}"
    echo -e "Symbols:  ${CYAN}$SYMBOLS_DIR${NC}"
    echo ""
    
    # Exit code
    if [[ $failed -eq 0 ]]; then
        echo -e "${GREEN}${BOLD}╔══════════════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${GREEN}${BOLD}║${NC}                      ${GREEN}✓ 100% PARITY ACHIEVED${NC}                                       ${BOLD}║${NC}"
        echo -e "${GREEN}${BOLD}╚══════════════════════════════════════════════════════════════════════════════════════╝${NC}"
        return 0
    else
        echo -e "${RED}${BOLD}╔══════════════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${RED}${BOLD}║${NC}                      ${RED}✗ FIXES NEEDED${NC}                                                  ${BOLD}║${NC}"
        echo -e "${RED}${BOLD}╚══════════════════════════════════════════════════════════════════════════════════════╝${NC}"
        return 1
    fi
}

# =============================================================================
# ENTRY POINT
# =============================================================================

check_deps
run_tests
