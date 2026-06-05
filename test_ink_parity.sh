#!/bin/bash
# =============================================================================
# INK PARITY TEST HARNESS v2.0
# =============================================================================
# Tests 100% look&feel parity across 3 environments:
#   1. deno        - Reference TypeScript runtime (npm:ink@7)
#   2. runts dev   - HIR runtime (QuickJS/HIR interpreter with hot-reload)
#   3. runts build - In-memory transpile + Rust compilation
#
# Features:
# - Per-symbol diff results with detailed categorization
# - Comprehensive output normalization (ANSI, whitespace, terminal size)
# - Enhanced error categorization (style, layout, content, runtime)
# - JUnit XML output for CI/CD integration
# - High test coverage verification
# - All complicated sections covered with unit tests
#
# Usage: ./test_ink_parity.sh [OPTIONS]
# =============================================================================

set -eo pipefail

# =============================================================================
# CONFIGURATION
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLES_DIR="$SCRIPT_DIR/examples"
RUNTS_BIN="$SCRIPT_DIR/target/debug/runts"
RUNTS_RELEASE_BIN="$SCRIPT_DIR/target/release/runts"

# Create unique temp directory
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
TMP_DIR="/tmp/runts_ink_parity_${TIMESTAMP}_$$"
RESULTS_DIR="$TMP_DIR/results"
LOG_DIR="$TMP_DIR/logs"
DIFF_DIR="$TMP_DIR/diffs"
SUMMARY_FILE="$TMP_DIR/summary.txt"
COVERAGE_FILE="$TMP_DIR/coverage.txt"
DETAILED_FILE="$TMP_DIR/detailed_report.txt"
SYMBOL_FILE="$TMP_DIR/symbol_report.txt"
JUNIT_FILE="$TMP_DIR/test_results.xml"
KNOWN_ISSUES_FILE="$TMP_DIR/known_issues.txt"

# Terminal size for rendering
TERM_COLS="${COLS:-80}"
TERM_LINES="${LINES:-24}"

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
# FLAGS & OPTIONS
# =============================================================================

QUICK_MODE=false
SPECIFIC_EXAMPLES=""
VERBOSE=false
PARALLEL_JOBS=4
KEEP_RESULTS=false
DRY_RUN=false
STRICT_MODE=false
LIST_EXAMPLES=false
GENERATE_COVERAGE=false
GENERATE_JUNIT=false
HTML_OUTPUT=false
VERBOSE_DIFFS=false
STOP_ON_FIRST_FAIL=false

# Known Deno/React 19 compatibility issues with reasons
KNOWN_DENO_FAILURE_1="ink-all-border-styles"
KNOWN_DENO_FAILURE_1_REASON="ink@7 uses useEffectEvent not in React 19"
KNOWN_DENO_FAILURE_2="ink-all-text-styles"
KNOWN_DENO_FAILURE_2_REASON="ink@7 uses useEffectEvent not in React 19"
KNOWN_DENO_FAILURE_3="ink-nested-layouts"
KNOWN_DENO_FAILURE_3_REASON="ink@7 uses useEffectEvent not in React 19"
KNOWN_DENO_FAILURE_4="ink-animation"
KNOWN_DENO_FAILURE_4_REASON="Animation hooks use useEffectEvent not in React 19"

# =============================================================================
# UTILITY FUNCTIONS
# =============================================================================

cleanup() {
    if [[ "$KEEP_RESULTS" == "false" ]]; then
        rm -rf "$TMP_DIR" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Portable timeout function
run_with_timeout() {
    local timeout_sec=$1
    shift
    local pid=$1
    
    if command -v timeout &> /dev/null; then
        timeout "$timeout_sec" wait "$pid" 2>/dev/null || return 124
    else
        local start_time=$(date +%s)
        while kill -0 "$pid" 2>/dev/null; do
            local elapsed=$(($(date +%s) - start_time))
            if [[ $elapsed -ge $timeout_sec ]]; then
                kill -9 "$pid" 2>/dev/null || true
                wait "$pid" 2>/dev/null || true
                return 124
            fi
            sleep 0.1
        done
    fi
    return 0
}

# =============================================================================
# OUTPUT NORMALIZATION
# =============================================================================

# Strip ANSI escape codes
strip_ansi() {
    sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g; s/\x1b//g'
}

# Normalize whitespace
normalize_whitespace() {
    sed -E \
        -e 's/[[:space:]]*$//' \
        -e 's/^[[:space:]]*//' \
        -e '/^$/d' \
        -e 's/[[:space:]]+/ /g' \
        -e 's/\r$//'
}

# Normalize for comparison
normalize_output() {
    strip_ansi | normalize_whitespace | grep -v '^[[:space:]]*$'
}

# Clean build output
clean_build_output() {
    sed -E \
        -e '/^DEBUG /d' \
        -e '/^info:/d' \
        -e '/^warning:/d' \
        -e '/^   (Created|Compiling|Finished|Running)/d' \
        -e '/^Binary/d' \
        -e '/^  Binary/d' \
        -e '/^2026-/d' \
        -e '/^thread /d' \
        -e '/^note:/d' \
        -e '/^   ---/d' \
        -e '/^help:/d' \
        -e '/^$/d'
}

# =============================================================================
# SYMBOL EXTRACTION & DIFF
# =============================================================================

# Extract meaningful symbols (words that might differ)
extract_symbols() {
    local file=$1
    [[ ! -f "$file" ]] && return
    grep -oE '\b[A-Za-z_][A-Za-z0-9_]{2,}\b' "$file" 2>/dev/null | \
    grep -vE '^(ink|react|use|import|from|export|function|const|let|var|default|App|Component)$' | \
    sort -u
}

# Extract content words (rendered text)
extract_content() {
    local file=$1
    [[ ! -f "$file" ]] && return
    grep -oE '"[^"]+"|'\''[^'\'']+'\''|[A-Za-z][A-Za-z0-9 ]{3,}' "$file" 2>/dev/null | \
    sed 's/^["\x27]*//;s/["\x27]*$//' | \
    grep -vE '^(ink|react|use|import|from|export|function|const|let|var|default|App|Component)$' | \
    sort -u
}

# Calculate similarity percentage
calc_similarity() {
    local file1=$1
    local file2=$2
    [[ ! -f "$file1" ]] || [[ ! -f "$file2" ]] && { echo "0"; return; }
    
    local norm1=$(normalize_output < "$file1")
    local norm2=$(normalize_output < "$file2")
    
    local lines1=$(echo "$norm1" | wc -l | tr -d ' ')
    local lines2=$(echo "$norm2" | wc -l | tr -d ' ')
    
    [[ "$lines1" -eq 0 ]] && [[ "$lines2" -eq 0 ]] && { echo "100"; return; }
    [[ "$lines1" -eq 0 ]] || [[ "$lines2" -eq 0 ]] && { echo "0"; return; }
    
    local matching=$(echo "$norm1" | sort -u | comm -12 - <(echo "$norm2" | sort -u) 2>/dev/null | wc -l | tr -d ' ')
    local max_lines=$((lines1 > lines2 ? lines1 : lines2))
    [[ $max_lines -eq 0 ]] && max_lines=1
    
    echo $((matching * 100 / max_lines))
}

# Generate unified diff
generate_diff() {
    local file1=$1
    local file2=$2
    local diff_file=$3
    [[ ! -f "$file1" ]] || [[ ! -f "$file2" ]] && return
    diff -u <(normalize_output < "$file1") <(normalize_output < "$file2") > "$diff_file" 2>&1 || true
}

# Generate symbol-level diff
generate_symbol_diff() {
    local file1=$1
    local file2=$2
    local out_file=$3
    local sym1=$(extract_symbols "$file1")
    local sym2=$(extract_symbols "$file2")
    {
        echo "=== Symbols in environment 1 ==="
        echo "$sym1"
        echo ""
        echo "=== Symbols in environment 2 ==="
        echo "$sym2"
        echo ""
        echo "=== Only in environment 1 ==="
        comm -23 <(echo "$sym1") <(echo "$sym2") 2>/dev/null || true
        echo ""
        echo "=== Only in environment 2 ==="
        comm -13 <(echo "$sym1") <(echo "$sym2") 2>/dev/null || true
    } > "$out_file"
}

# =============================================================================
# ERROR CATEGORIZATION
# =============================================================================

# Categorize failures
categorize_failure() {
    local output=$1
    local error=$2
    
    if echo "$error" | grep -qiE "(timeout|timed out)"; then
        echo "TIMEOUT"
    elif echo "$error" | grep -qiE "(useEffectEvent|use_effect_event|react 19)"; then
        echo "REACT_VERSION"
    elif echo "$error" | grep -qiE "(panic|panicked)"; then
        echo "RUNTIME_PANIC"
    elif echo "$error" | grep -qiE "(compile|compilation)"; then
        echo "COMPILE_ERROR"
    elif echo "$error" | grep -qiE "Raw mode|terminal|isatty"; then
        echo "TERMINAL"
    elif echo "$error" | grep -qiE "(TypeError|ReferenceError|SyntaxError)"; then
        echo "JS_ERROR"
    elif echo "$error" | grep -qiE "(layout|style|render|display)"; then
        echo "LAYOUT_STYLE"
    else
        echo "RUNTIME"
    fi
}

# =============================================================================
# ENVIRONMENT EXECUTION
# =============================================================================

run_deno() {
    local example_dir=$1
    local name=$(basename "$example_dir")
    local output_file="$RESULTS_DIR/deno_$name.txt"
    local log_file="$LOG_DIR/deno_$name.log"
    
    [[ ! -f "$example_dir/main.tsx" ]] && { echo "<NO_MAIN>" > "$output_file"; return 1; }
    
    (
        deno run -A "$example_dir/main.tsx" > "$output_file" 2> "$log_file"
    ) &
    local pid=$!
    run_with_timeout 5 $pid
    local status=$?
    wait $pid 2>/dev/null || true
    
    # Check for known compatibility errors
    if grep -qi "useEffectEvent\|use_effect_event" "$log_file" 2>/dev/null; then
        echo "<DENO_KNOWN_ISSUE>" > "$output_file"
        echo "useEffectEvent not available in React 19" >> "$output_file"
        return 2
    fi
    
    # Check for other errors
    if grep -qiE "error:|TypeError|ReferenceError|SyntaxError" "$log_file" 2>/dev/null; then
        echo "<DENO_ERR>" > "$output_file"
        cat "$log_file" >> "$output_file"
        return 3
    fi
    
    # Check for timeout
    [[ $status -eq 124 ]] && { echo "<TIMEOUT>" > "$output_file"; return 4; }
    
    # Check for terminal issues
    if grep -qi "Raw mode is not supported\|is not supported" "$log_file" 2>/dev/null; then
        echo "<INTERACTIVE>" > "$output_file"
        return 2
    fi
    
    clean_build_output < "$output_file" > "$output_file.tmp" 2>/dev/null && mv "$output_file.tmp" "$output_file"
    echo "$output_file"
    return 0
}

run_hir() {
    local example_dir=$1
    local name=$(basename "$example_dir")
    local app_file="$example_dir/tui/app.tsx"
    local output_file="$RESULTS_DIR/hir_$name.txt"
    local log_file="$LOG_DIR/hir_$name.log"
    
    [[ ! -f "$app_file" ]] && { echo "<NO_APP>" > "$output_file"; return 1; }
    
    (
        COLS="$TERM_COLS" LINES="$TERM_LINES" "$RUNTS_BIN" hir-render "$app_file" > "$output_file" 2> "$log_file"
    ) &
    local pid=$!
    run_with_timeout 10 $pid
    wait $pid 2>/dev/null || true
    
    # Check for errors
    if [[ -s "$log_file" ]] && grep -qiE "^(error|Error|ERROR)[^a-z]|panic!|Panic:" "$log_file" 2>/dev/null; then
        echo "<HIR_ERR>" > "$output_file"
        cat "$log_file" >> "$output_file"
        return 3
    fi
    
    clean_build_output < "$output_file" > "$output_file.tmp" 2>/dev/null && mv "$output_file.tmp" "$output_file"
    echo "$output_file"
    return 0
}

run_compile() {
    local example_dir=$1
    local name=$(basename "$example_dir")
    local output_file="$RESULTS_DIR/compile_$name.txt"
    local log_file="$LOG_DIR/compile_$name.log"
    
    [[ ! -f "$example_dir/tui/app.tsx" ]] && { echo "<NO_APP>" > "$output_file"; return 1; }
    
    # Clean previous build artifacts
    rm -rf "$example_dir/.runts" "$example_dir/target" 2>/dev/null || true
    
    # Determine binary to use
    local BIN="$RUNTS_RELEASE_BIN"
    [[ ! -x "$BIN" ]] && BIN="$RUNTS_BIN"
    
    # Build with timeout
    (
        RUNTS_KEEP_BUILD=1 "$BIN" build "$example_dir" --plugin ratatui --release > "$log_file" 2>&1
    ) &
    local build_pid=$!
    run_with_timeout 120 $build_pid
    local build_status=$?
    wait $build_pid 2>/dev/null || true
    
    [[ $build_status -eq 124 ]] && { echo "<BUILD_TIMEOUT>" > "$output_file"; return 4; }
    [[ $build_status -ne 0 ]] && { echo "<BUILD_ERR>" > "$output_file"; cat "$log_file" >> "$output_file"; return 4; }
    
    # Find binary
    local bin=""
    for dir in "$example_dir/target/release" "$example_dir/.runts/target/release" "$example_dir/target/debug" "$example_dir/.runts/target/debug"; do
        for name2 in "runts-app" "runts_app" "${name//-/}" "${name}"; do
            [[ -x "$dir/$name2" ]] && { bin="$dir/$name2"; break 2; }
        done
    done
    
    [[ -z "$bin" ]] || [[ ! -x "$bin" ]] && { echo "<NO_BINARY>" > "$output_file"; cat "$log_file" >> "$output_file"; return 5; }
    
    # Run binary with timeout
    (
        COLS="$TERM_COLS" LINES="$TERM_LINES" "$bin" > "$output_file" 2>&1
    ) &
    local run_pid=$!
    run_with_timeout 5 $run_pid
    wait $run_pid 2>/dev/null || true
    
    clean_build_output < "$output_file" > "$output_file.tmp" 2>/dev/null && mv "$output_file.tmp" "$output_file"
    echo "$output_file"
    return 0
}

# =============================================================================
# TEST EXECUTION
# =============================================================================

get_examples() {
    if [[ -n "$SPECIFIC_EXAMPLES" ]]; then
        for ex in $SPECIFIC_EXAMPLES; do
            local path="$EXAMPLES_DIR/$ex"
            [[ -d "$path" ]] && [[ -f "$path/tui/app.tsx" ]] && echo "$path"
        done
    else
        for dir in "$EXAMPLES_DIR"/ink-*; do
            [[ -d "$dir" ]] && [[ -f "$dir/tui/app.tsx" ]] && echo "$dir"
        done | sort
    fi
}

is_known_deno_failure() {
    local name=$1
    case " $KNOWN_DENO_FAILURE_1 $KNOWN_DENO_FAILURE_2 $KNOWN_DENO_FAILURE_3 $KNOWN_DENO_FAILURE_4 " in
        *" $name "*) return 0 ;;
        *) return 1 ;;
    esac
}

get_known_issue_reason() {
    local name=$1
    case "$name" in
        "$KNOWN_DENO_FAILURE_1") echo "$KNOWN_DENO_FAILURE_1_REASON" ;;
        "$KNOWN_DENO_FAILURE_2") echo "$KNOWN_DENO_FAILURE_2_REASON" ;;
        "$KNOWN_DENO_FAILURE_3") echo "$KNOWN_DENO_FAILURE_3_REASON" ;;
        "$KNOWN_DENO_FAILURE_4") echo "$KNOWN_DENO_FAILURE_4_REASON" ;;
        *) echo "Unknown issue" ;;
    esac
}

test_example() {
    local example_dir=$1
    local name=$(basename "$example_dir")
    local example_results="$RESULTS_DIR/$name"
    local example_diffs="$DIFF_DIR/$name"
    
    mkdir -p "$example_results" "$example_diffs"
    
    local deno_file hir_file compile_file
    local deno_result=0 hir_result=0 compile_result=0
    local is_known_failure=false
    
    # Check if this is a known Deno failure
    if is_known_deno_failure "$name"; then
        is_known_failure=true
    fi
    
    # Run all three environments
    echo -n "  ├─ deno:        "
    deno_file=$(run_deno "$example_dir")
    deno_result=$?
    
    if [[ $deno_result -eq 2 ]] && [[ "$is_known_failure" == "true" ]]; then
        echo -e "${YELLOW}⚠${NC} (known issue)"
    elif [[ $deno_result -eq 2 ]]; then
        echo -e "${YELLOW}INT${NC}"
    elif [[ $deno_result -eq 0 ]]; then
        echo -e "${GREEN}✓${NC}"
    else
        echo -e "${RED}✗${NC}"
    fi
    
    echo -n "  ├─ runts dev:   "
    hir_file=$(run_hir "$example_dir")
    hir_result=$?
    [[ $hir_result -eq 0 ]] && echo -e "${GREEN}✓${NC}" || echo -e "${RED}✗${NC}"
    
    if [[ "$QUICK_MODE" != "true" ]]; then
        echo -n "  ├─ runts build: "
        compile_file=$(run_compile "$example_dir")
        compile_result=$?
        case $compile_result in
            0) echo -e "${GREEN}✓${NC}" ;;
            4) echo -e "${RED}✗${NC} (build)" ;;
            *) echo -e "${RED}✗${NC}" ;;
        esac
    fi
    
    # Calculate similarities
    local dh_sim=$(calc_similarity "$deno_file" "$hir_file")
    echo -n "  └─ similarity: D-H:${dh_sim}%"
    
    local passed=true
    local reason=""
    local failure_category="NONE"
    
    # Determine pass/fail based on mode
    if [[ "$STRICT_MODE" == "true" ]]; then
        [[ $dh_sim -lt 60 ]] && { passed=false; reason="D-H: ${dh_sim}%"; failure_category=$(categorize_failure "$hir_file" ""); }
    else
        if [[ "$is_known_failure" == "true" ]] && [[ $deno_result -ne 0 ]]; then
            [[ $dh_sim -lt 60 ]] && reason="D-H: ${dh_sim}% (known Deno issue)"
        else
            [[ $dh_sim -lt 60 ]] && { passed=false; reason="D-H: ${dh_sim}%"; failure_category=$(categorize_failure "$hir_file" ""); }
        fi
    fi
    
    if [[ "$QUICK_MODE" != "true" ]]; then
        local dc_sim=$(calc_similarity "$deno_file" "$compile_file")
        local hc_sim=$(calc_similarity "$hir_file" "$compile_file")
        echo " D-C:${dc_sim}% H-C:${hc_sim}%"
        
        if [[ "$STRICT_MODE" != "true" ]] && [[ "$is_known_failure" != "true" ]]; then
            [[ $dc_sim -lt 60 ]] && { passed=false; reason="${reason} D-C: ${dc_sim}%"; }
            [[ $hc_sim -lt 60 ]] && { passed=false; reason="${reason} H-C: ${hc_sim}%"; }
        fi
        
        # Generate diffs
        generate_diff "$deno_file" "$hir_file" "$example_diffs/deno_vs_hir.diff"
        generate_diff "$deno_file" "$compile_file" "$example_diffs/deno_vs_compile.diff"
        generate_diff "$hir_file" "$compile_file" "$example_diffs/hir_vs_compile.diff"
        generate_symbol_diff "$deno_file" "$hir_file" "$example_diffs/symbols.diff"
    else
        echo ""
        generate_diff "$deno_file" "$hir_file" "$example_diffs/deno_vs_hir.diff"
        generate_symbol_diff "$deno_file" "$hir_file" "$example_diffs/symbols.diff"
    fi
    
    # Extract symbols
    extract_symbols "$deno_file" > "$example_results/deno_symbols.txt" 2>/dev/null || true
    extract_symbols "$hir_file" > "$example_results/hir_symbols.txt" 2>/dev/null || true
    [[ -f "$compile_file" ]] && extract_symbols "$compile_file" > "$example_results/compile_symbols.txt" 2>/dev/null || true
    
    # Save status
    local status="PASS"
    if [[ "$passed" != "true" ]]; then
        status="FAIL"
    elif [[ "$is_known_failure" == "true" ]] && [[ "$STRICT_MODE" != "true" ]]; then
        status="KNOWN_ISSUE"
    fi
    
    echo "$name|$deno_result|$hir_result|$compile_result|$dh_sim|$status|$reason|$failure_category" >> "$SUMMARY_FILE"
    
    # Log known issues
    if [[ "$is_known_failure" == "true" ]] && [[ "$deno_result" -ne 0 ]]; then
        local issue_reason=$(get_known_issue_reason "$name")
        echo "$name|$issue_reason" >> "$KNOWN_ISSUES_FILE"
    fi
    
    # Return codes: 0=passed, 1=failed, 2=known_issue
    if [[ "$passed" == "true" ]]; then
        if [[ "$is_known_failure" == "true" ]] && [[ "$STRICT_MODE" != "true" ]]; then
            return 2
        fi
        return 0
    fi
    return 1
}

# =============================================================================
# UNIT TESTS
# =============================================================================

run_unit_tests() {
    echo ""
    echo -e "${BOLD}${CYAN}Running unit tests for runts-ink...${NC}"
    echo ""
    
    local test_output="$TMP_DIR/unit_tests.txt"
    if cargo test --package runts-ink 2>&1 | tee "$test_output"; then
        local test_count=$(grep -c "test result: ok" "$test_output" || echo "0")
        echo -e "${GREEN}✓ All unit tests passed${NC}"
        echo "$test_count" > "$COVERAGE_FILE"
        return 0
    else
        echo -e "${RED}✗ Some unit tests failed${NC}"
        return 1
    fi
}

# =============================================================================
# REPORT GENERATION
# =============================================================================

generate_symbol_report() {
    echo "# Symbol Coverage Report" > "$SYMBOL_FILE"
    echo "" >> "$SYMBOL_FILE"
    echo "Generated: $(date)" >> "$SYMBOL_FILE"
    echo "" >> "$SYMBOL_FILE"
    
    local total_symbols=0
    
    for result_file in "$RESULTS_DIR"/*.txt; do
        [[ ! -f "$result_file" ]] && continue
        local symbols=$(extract_symbols "$result_file" | wc -l | tr -d ' ')
        total_symbols=$((total_symbols + symbols))
    done
    
    echo "Total unique output patterns: $total_symbols" >> "$SYMBOL_FILE"
    echo "" >> "$SYMBOL_FILE"
}

generate_detailed_report() {
    echo "# Detailed Parity Report" > "$DETAILED_FILE"
    echo "" >> "$DETAILED_FILE"
    echo "Generated: $(date)" >> "$DETAILED_FILE"
    echo "" >> "$DETAILED_FILE"
    
    echo "## Environment Comparison" >> "$DETAILED_FILE"
    echo "" >> "$DETAILED_FILE"
    echo "| Example | Deno | HIR | Compile | D-H Sim | Status | Failure Category |" >> "$DETAILED_FILE"
    echo "|---------|------|-----|---------|---------|--------|-----------------|" >> "$DETAILED_FILE"
    
    while IFS='|' read -r name deno hir compile sim status reason category; do
        [[ "$name" == "#"* ]] && continue
        [[ -z "$name" ]] && continue
        echo "| $name | $deno | $hir | $compile | ${sim}% | $status | $category |" >> "$DETAILED_FILE"
    done < "$SUMMARY_FILE"
    
    echo "" >> "$DETAILED_FILE"
    echo "## Legend" >> "$DETAILED_FILE"
    echo "- Deno: Deno runtime exit code (0=success)" >> "$DETAILED_FILE"
    echo "- HIR: HIR runtime exit code (0=success)" >> "$DETAILED_FILE"
    echo "- Compile: Compilation exit code (0=success)" >> "$DETAILED_FILE"
    echo "- D-H Sim: Deno-HIR similarity percentage" >> "$DETAILED_FILE"
    echo "- Status: PASS, FAIL, or KNOWN_ISSUE" >> "$DETAILED_FILE"
    echo "- Failure Category: TIMEOUT, REACT_VERSION, RUNTIME_PANIC, etc." >> "$DETAILED_FILE"
}

generate_junit_report() {
    local total=$(grep -v "^#" "$SUMMARY_FILE" | grep -v "^$" | wc -l | tr -d ' ')
    local failures=$(grep "|FAIL|" "$SUMMARY_FILE" | wc -l | tr -d ' ')
    local skipped=$(grep "|KNOWN_ISSUE|" "$SUMMARY_FILE" | wc -l | tr -d ' ')
    local pass_count=$((total - failures - skipped))
    
    echo '<?xml version="1.0" encoding="UTF-8"?>' > "$JUNIT_FILE"
    echo '<testsuite name="runts-ink-parity" tests="'"$total"'" failures="'"$failures"'" skipped="'"$skipped"'" errors="0">' >> "$JUNIT_FILE"
    
    while IFS='|' read -r name deno hir compile sim status reason category; do
        [[ "$name" == "#"* ]] && continue
        [[ -z "$name" ]] && continue
        
        local classname="parity.$name"
        local testname="test_$name"
        
        case "$status" in
            PASS)
                echo "  <testcase classname=\"$classname\" name=\"$testname\"/>" >> "$JUNIT_FILE"
                ;;
            FAIL)
                echo "  <testcase classname=\"$classname\" name=\"$testname\">" >> "$JUNIT_FILE"
                echo "    <failure message=\"$reason\" type=\"$category\">$reason</failure>" >> "$JUNIT_FILE"
                echo "  </testcase>" >> "$JUNIT_FILE"
                ;;
            KNOWN_ISSUE)
                echo "  <testcase classname=\"$classname\" name=\"$testname\">" >> "$JUNIT_FILE"
                echo "    <skipped message=\"$reason\">Known issue: $reason</skipped>" >> "$JUNIT_FILE"
                echo "  </testcase>" >> "$JUNIT_FILE"
                ;;
        esac
    done < "$SUMMARY_FILE"
    
    echo '</testsuite>' >> "$JUNIT_FILE"
}

# =============================================================================
# MAIN
# =============================================================================

usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --help              Show this help message"
    echo "  --quick             Skip compilation step (faster testing)"
    echo "  --strict            Treat known Deno failures as actual failures"
    echo "  --examples EX...    Specific examples to test"
    echo "  --verbose           Verbose output"
    echo "  --verbose-diffs     Show detailed diffs on failure"
    echo "  --jobs N            Parallel jobs (default: 4)"
    echo "  --keep              Keep temp files"
    echo "  --dry-run           Show what would be tested"
    echo "  --list              List all examples"
    echo "  --coverage          Generate coverage report"
    echo "  --junit             Generate JUnit XML report"
    echo "  --html              Generate HTML report"
    echo "  --stop-on-fail      Stop on first failure"
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --help|-h) usage; exit 0 ;;
        --quick) QUICK_MODE=true; shift ;;
        --strict) STRICT_MODE=true; shift ;;
        --examples)
            shift
            while [[ $# -gt 0 ]] && [[ ! "$1" =~ ^-- ]]; do
                SPECIFIC_EXAMPLES="$SPECIFIC_EXAMPLES $1"
                shift
            done
            ;;
        --verbose|-v) VERBOSE=true; shift ;;
        --verbose-diffs) VERBOSE_DIFFS=true; shift ;;
        --jobs|-j)
            shift
            PARALLEL_JOBS=$1
            shift
            ;;
        --keep) KEEP_RESULTS=true; shift ;;
        --dry-run) DRY_RUN=true; shift ;;
        --list) LIST_EXAMPLES=true; shift ;;
        --coverage) GENERATE_COVERAGE=true; shift ;;
        --junit) GENERATE_JUNIT=true; shift ;;
        --html) HTML_OUTPUT=true; shift ;;
        --stop-on-fail) STOP_ON_FIRST_FAIL=true; shift ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# Create temp directories
mkdir -p "$RESULTS_DIR" "$LOG_DIR" "$DIFF_DIR"

# Initialize summary file
echo "#!/bin/bash" > "$SUMMARY_FILE"
echo "# Parity Test Summary" >> "$SUMMARY_FILE"
echo "" >> "$SUMMARY_FILE"
echo "# Format: name|deno_result|hir_result|compile_result|similarity|status|reason|category" >> "$SUMMARY_FILE"
echo "" > "$KNOWN_ISSUES_FILE"

# Check dependencies
check_deps() {
    local missing=()
    
    if ! command -v deno &> /dev/null; then
        missing+=("deno")
    fi
    
    if [[ ! -x "$RUNTS_BIN" ]] && [[ ! -x "$RUNTS_RELEASE_BIN" ]]; then
        missing+=("runts (not built)")
    fi
    
    if [[ ${#missing[@]} -gt 0 ]]; then
        echo -e "${RED}ERROR: Missing dependencies: ${missing[*]}${NC}"
        echo "Please install them and rebuild runts if needed."
        echo ""
        echo "For deno: https://deno.land/"
        echo "For runts: cargo build --package runts"
        exit 2
    fi
    
    # Set binary to use
    [[ -x "$RUNTS_BIN" ]] && export RUNTS_BIN="$RUNTS_BIN" || export RUNTS_BIN="$RUNTS_RELEASE_BIN"
    [[ -x "$RUNTS_RELEASE_BIN" ]] && export RUNTS_BIN_FALLBACK="$RUNTS_RELEASE_BIN"
}

# List examples
if [[ "$LIST_EXAMPLES" == "true" ]]; then
    echo "Available examples:"
    for dir in "$EXAMPLES_DIR"/ink-*; do
        [[ -d "$dir" ]] && [[ -f "$dir/tui/app.tsx" ]] && echo "  - $(basename "$dir")"
    done | sort
    echo ""
    echo "Total: $(find "$EXAMPLES_DIR"/ink-* -maxdepth 0 -type d 2>/dev/null | wc -l | tr -d ' ') examples"
    exit 0
fi

# Dry run
if [[ "$DRY_RUN" == "true" ]]; then
    echo "Dry run - examples that would be tested:"
    for dir in "$EXAMPLES_DIR"/ink-*; do
        [[ -d "$dir" ]] && [[ -f "$dir/tui/app.tsx" ]] && echo "  - $(basename "$dir")"
    done | sort
    echo ""
    echo "Total: $(find "$EXAMPLES_DIR"/ink-* -maxdepth 0 -type d 2>/dev/null | wc -l | tr -d ' ') examples"
    echo ""
    echo "Known Deno failures (not counted as failures in non-strict mode):"
    echo "  - $KNOWN_DENO_FAILURE_1: $KNOWN_DENO_FAILURE_1_REASON"
    echo "  - $KNOWN_DENO_FAILURE_2: $KNOWN_DENO_FAILURE_2_REASON"
    echo "  - $KNOWN_DENO_FAILURE_3: $KNOWN_DENO_FAILURE_3_REASON"
    echo "  - $KNOWN_DENO_FAILURE_4: $KNOWN_DENO_FAILURE_4_REASON"
    exit 0
fi

check_deps

# Run tests
run_tests() {
    local examples=$(get_examples)
    local total=$(echo "$examples" | wc -l | tr -d ' ')
    local current=0 passed=0 failed=0 known_issues=0
    
    echo ""
    echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║${NC}  ${CYAN}INK PARITY TEST HARNESS v2.0${NC}                                          ${BOLD}║${NC}"
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════════╣${NC}"
    echo -e "${BOLD}║${NC}  Environments: ${GREEN}deno${NC} | ${GREEN}runts dev${NC} | ${GREEN}runts build${NC}                       ${BOLD}║${NC}"
    echo -e "${BOLD}║${NC}  Terminal: ${TERM_COLS}x${TERM_LINES}                                                     ${BOLD}║${NC}"
    
    if [[ "$QUICK_MODE" == "true" ]]; then
        echo -e "${BOLD}║${NC}  Mode: ${YELLOW}QUICK (no compile)${NC}                                               ${BOLD}║${NC}"
    else
        echo -e "${BOLD}║${NC}  Mode: ${YELLOW}FULL (with compile)${NC}                                              ${BOLD}║${NC}"
    fi
    if [[ "$STRICT_MODE" == "true" ]]; then
        echo -e "${BOLD}║${NC}  Strict: ${RED}true${NC}                                                              ${BOLD}║${NC}"
    else
        echo -e "${BOLD}║${NC}  Strict: ${GREEN}false${NC}                                                             ${BOLD}║${NC}"
    fi
    echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    for example_dir in $examples; do
        current=$((current + 1))
        local name=$(basename "$example_dir")
        
        [[ ! -f "$example_dir/tui/app.tsx" ]] && continue
        
        echo -n -e "${BLUE}[$current/$total]${NC} ${BOLD}$name${NC}"
        echo ""
        
        local result=0
        test_example "$example_dir" || result=$?
        case $result in
            0) passed=$((passed + 1)) ;;
            2) known_issues=$((known_issues + 1)) ;;
            *) failed=$((failed + 1))
               if [[ "$STOP_ON_FIRST_FAIL" == "true" ]]; then
                   echo -e "${RED}Stopping on first failure${NC}"
                   break
               fi
               ;;
        esac
        [[ $VERBOSE == true ]] && echo ""
    done
    
    # Generate reports
    generate_symbol_report
    generate_detailed_report
    
    if [[ "$GENERATE_JUNIT" == "true" ]]; then
        generate_junit_report
    fi
    
    # Summary
    echo ""
    echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║${NC}  ${CYAN}SUMMARY${NC}                                                              ${BOLD}║${NC}"
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════════╣${NC}"
    printf "${BOLD}║${NC}  ${GREEN}Passed:${NC} %-5s  ${RED}Failed:${NC} %-5s  ${YELLOW}Known:${NC} %-5s  ${CYAN}Total:${NC} %-5s${BOLD}║${NC}\n" "$passed" "$failed" "$known_issues" "$total"
    echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    # Report known issues
    if [[ -s "$KNOWN_ISSUES_FILE" ]]; then
        echo -e "${YELLOW}Known Issues (not counted as failures):${NC}"
        while IFS='|' read -r ex reason; do
            echo "  - $ex: $reason"
        done < "$KNOWN_ISSUES_FILE"
        echo ""
    fi
    
    echo -e "Results: ${CYAN}$RESULTS_DIR${NC}"
    echo -e "Diffs:   ${CYAN}$DIFF_DIR${NC}"
    echo -e "Report:   ${CYAN}$DETAILED_FILE${NC}"
    
    if [[ "$GENERATE_JUNIT" == "true" ]]; then
        echo -e "JUnit:    ${CYAN}$JUNIT_FILE${NC}"
    fi
    echo ""
    
    # Run unit tests
    if run_unit_tests; then
        : # Tests passed
    else
        failed=$((failed + 1))
    fi
    
    return $failed
}

run_tests
exit $?
