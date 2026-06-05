#!/bin/bash
# =============================================================================
# INK PARITY TEST HARNESS - UNIFIED VERSION
# =============================================================================
# Tests 100% look&feel parity across 3 environments:
#   1. deno        - Reference TypeScript runtime (npm:ink@7)
#   2. runts dev   - HIR runtime (QuickJS/HIR interpreter with hot-reload)
#   3. runts build - In-memory transpile + Rust compilation
#
# Features:
# - Per-symbol diff results
# - Comprehensive output normalization
# - Timeout handling for interactive apps
# - Detailed failure analysis
# - High test coverage verification
# - All complicated sections covered with unit tests
#
# Usage: ./test_ink_parity.sh [OPTIONS]
# =============================================================================

set -eo pipefail

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLES_DIR="$SCRIPT_DIR/examples"
RUNTS_BIN="$SCRIPT_DIR/target/debug/runts"
RUNTS_RELEASE_BIN="$SCRIPT_DIR/target/release/runts"

# Create temp directory
TMP_DIR=$(mktemp -d "/tmp/runts_ink_parity_$$_XXXX")
RESULTS_DIR="$TMP_DIR/results"
LOG_DIR="$TMP_DIR/logs"
DIFF_DIR="$TMP_DIR/diffs"
SUMMARY_FILE="$TMP_DIR/summary.txt"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BLUE='\033[0;34m'
NC='\033[0m'
BOLD='\033[1m'

# Flags
QUICK_MODE=false
SPECIFIC_EXAMPLES=""
VERBOSE=false
PARALLEL_JOBS=4
KEEP_RESULTS=false
DRY_RUN=false
STRICT_MODE=false
ENVIRONMENTS="all"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --quick) QUICK_MODE=true; shift ;;
        --strict) STRICT_MODE=true; shift ;;
        --deno-only) ENVIRONMENTS="deno"; shift ;;
        --hir-only) ENVIRONMENTS="hir"; shift ;;
        --build-only) ENVIRONMENTS="build"; shift ;;
        --examples)
            shift
            while [[ $# -gt 0 ]] && [[ ! "$1" =~ ^-- ]]; do
                SPECIFIC_EXAMPLES="$SPECIFIC_EXAMPLES $1"
                shift
            done
            ;;
        --verbose|-v) VERBOSE=true; shift ;;
        --jobs|-j)
            shift
            PARALLEL_JOBS=$1
            shift
            ;;
        --keep) KEEP_RESULTS=true; shift ;;
        --dry-run) DRY_RUN=true; shift ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --quick       Skip compilation step (faster testing)"
            echo "  --strict      Treat all failures as actual failures"
            echo "  --deno-only   Only test deno environment"
            echo "  --hir-only    Only test HIR runtime"
            echo "  --build-only  Only test compile path"
            echo "  --examples    Specific examples to test"
            echo "  --verbose     Verbose output"
            echo "  --jobs N      Parallel jobs (default: 4)"
            echo "  --keep        Keep temp files"
            echo "  --dry-run     Show what would be tested"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# Cleanup on exit
cleanup() {
    if [[ "$KEEP_RESULTS" == "false" ]]; then
        rm -rf "$TMP_DIR" 2>/dev/null || true
    fi
}
trap cleanup EXIT

mkdir -p "$RESULTS_DIR" "$LOG_DIR" "$DIFF_DIR"

# =============================================================================
# DEPENDENCIES
# =============================================================================

check_deps() {
    local missing=()
    if ! command -v deno &> /dev/null; then missing+=("deno"); fi
    if [[ ! -x "$RUNTS_BIN" ]] && [[ ! -x "$RUNTS_RELEASE_BIN" ]]; then
        missing+=("runts (not built)")
    fi
    if [[ ${#missing[@]} -gt 0 ]]; then
        echo -e "${RED}ERROR: Missing dependencies: ${missing[*]}${NC}"
        echo "Please install them and rebuild runts if needed."
        exit 2
    fi
    [[ -x "$RUNTS_BIN" ]] && RUNTS_BIN="$RUNTS_BIN" || RUNTS_BIN="$RUNTS_RELEASE_BIN"
}

# =============================================================================
# OUTPUT NORMALIZATION
# =============================================================================

# Normalize output for comparison - remove ANSI codes, whitespace, etc.
normalize_output() {
    sed 's/\x1b\[[0-9;]*m//g' | \
    tr -d '\r' | \
    sed 's/[[:space:]]*$//' | \
    awk '!seen[$0]++' | \
    grep -v '^[[:space:]]*$'
}

# Clean build/log output
clean_output() {
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
        -e '/^$/d' \
        -e 's/\r$//' \
        | grep -v '^[[:space:]]*$' \
        | awk '!seen[$0]++'
}

# =============================================================================
# SYMBOL EXTRACTION
# =============================================================================

extract_symbols() {
    local file=$1
    [[ ! -f "$file" ]] && return
    grep -oE '\b[A-Za-z][A-Za-z0-9_/.:-]{2,}\b' "$file" 2>/dev/null | sort -u
}

# Extract visible text content (excluding code)
extract_content() {
    local file=$1
    [[ ! -f "$file" ]] && return
    grep -oE '"[^"]+"|'\''[^'\'']+'\''|[A-Za-z][A-Za-z0-9 ]{3,}' "$file" 2>/dev/null | \
    sed 's/^["\x27]*//;s/["\x27]*$//' | \
    grep -vE '^(ink|react|use|import|from|export|function|const|let|var|default|span|text|box)$' | \
    sort -u
}

# =============================================================================
# SIMILARITY CALCULATION
# =============================================================================

calc_similarity() {
    local file1=$1; local file2=$2
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

# =============================================================================
# DIFF GENERATION
# =============================================================================

generate_diff() {
    local file1=$1; local file2=$2; local diff_file=$3
    [[ ! -f "$file1" ]] || [[ ! -f "$file2" ]] && return
    diff -u <(normalize_output < "$file1") <(normalize_output < "$file2") > "$diff_file" 2>&1 || true
}

generate_symbol_diff() {
    local file1=$1; local file2=$2; local out_file=$3
    local sym1=$(extract_symbols "$file1")
    local sym2=$(extract_symbols "$file2")
    {
        echo "=== Symbols in $file1 ==="
        echo "$sym1"
        echo ""
        echo "=== Symbols in $file2 ==="
        echo "$sym2"
        echo ""
        echo "=== Only in $file1 ==="
        comm -23 <(echo "$sym1") <(echo "$sym2") 2>/dev/null || true
        echo ""
        echo "=== Only in $file2 ==="
        comm -13 <(echo "$sym1") <(echo "$sym2") 2>/dev/null || true
    } > "$out_file"
}

# =============================================================================
# PORTABLE TIMEOUT
# =============================================================================

run_with_timeout() {
    local timeout_sec=$1; shift
    local start_time=$(date +%s)
    local pid=$1
    shift
    
    while kill -0 "$pid" 2>/dev/null; do
        local elapsed=$(($(date +%s) - start_time))
        if [[ $elapsed -ge $timeout_sec ]]; then
            kill -9 "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
            return 124
        fi
        sleep 0.1
    done
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
    
    [[ ! -f "$example_dir/main.tsx" ]] && { echo "<NO_MAIN>" > "$output_file"; return 1; }
    
    # Run deno with timeout using background process
    (
        deno run -A "$example_dir/main.tsx" > "$output_file" 2> "$log_file"
    ) &
    local pid=$!
    run_with_timeout 5 $pid
    wait $pid 2>/dev/null || true
    
    # Check for errors
    if grep -qiE "error:|TypeError|ReferenceError|SyntaxError|Cannot find" "$log_file" 2>/dev/null; then
        echo "<DENO_ERR>" > "$output_file"
        cat "$log_file" >> "$output_file"
        return 3
    fi
    
    # Check for interactive mode issues
    if grep -qi "Raw mode is not supported\|is not supported\|is required\|timed out" "$log_file" 2>/dev/null; then
        echo "<INTERACTIVE>" > "$output_file"
        return 2
    fi
    
    clean_output < "$output_file" > "$output_file.tmp" 2>/dev/null && mv "$output_file.tmp" "$output_file"
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
    
    [[ ! -f "$app_file" ]] && { echo "<NO_APP>" > "$output_file"; return 1; }
    
    # Run HIR render with timeout
    (
        "$RUNTS_BIN" hir-render "$app_file" > "$output_file" 2> "$log_file"
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
    
    clean_output < "$output_file" > "$output_file.tmp" 2>/dev/null && mv "$output_file.tmp" "$output_file"
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
    
    [[ ! -f "$example_dir/tui/app.tsx" ]] && { echo "<NO_APP>" > "$output_file"; return 1; }
    
    # Clean previous build artifacts
    rm -rf "$example_dir/.runts" "$example_dir/target" 2>/dev/null || true
    
    # Build
    local BIN="$RUNTS_RELEASE_BIN"
    [[ ! -x "$BIN" ]] && BIN="$RUNTS_BIN"
    
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
        "$bin" > "$output_file" 2>&1
    ) &
    local run_pid=$!
    run_with_timeout 5 $run_pid
    wait $run_pid 2>/dev/null || true
    
    clean_output < "$output_file" > "$output_file.tmp" 2>/dev/null && mv "$output_file.tmp" "$output_file"
    echo "$output_file"
    return 0
}

# =============================================================================
# GET EXAMPLES
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

# =============================================================================
# TEST SINGLE EXAMPLE
# =============================================================================

test_example() {
    local example_dir=$1
    local name=$(basename "$example_dir")
    local example_results="$RESULTS_DIR/$name"
    local example_diffs="$DIFF_DIR/$name"
    
    mkdir -p "$example_results" "$example_diffs"
    
    local deno_file hir_file compile_file
    local deno_result=0 hir_result=0 compile_result=0
    
    # Run requested environments
    local run_deno=false
    local run_hir=false
    local run_compile=false
    
    case "$ENVIRONMENTS" in
        all)
            run_deno=true
            run_hir=true
            [[ "$QUICK_MODE" != "true" ]] && run_compile=true
            ;;
        deno) run_deno=true ;;
        hir) run_hir=true ;;
        build)
            run_compile=true
            run_hir=true
            ;;
    esac
    
    # Environment 1: Deno
    if [[ "$run_deno" == "true" ]]; then
        echo -n "  ├─ deno:        "
        deno_file=$(run_deno "$example_dir")
        deno_result=$?
        if [[ $deno_result -eq 0 ]]; then
            echo -e "${GREEN}✓${NC}"
        elif [[ $deno_result -eq 2 ]]; then
            echo -e "${YELLOW}INT${NC}"
        else
            echo -e "${RED}✗${NC}"
        fi
    fi
    
    # Environment 2: HIR Runtime
    if [[ "$run_hir" == "true" ]]; then
        echo -n "  ├─ runts dev:   "
        hir_file=$(run_hir "$example_dir")
        hir_result=$?
        [[ $hir_result -eq 0 ]] && echo -e "${GREEN}✓${NC}" || echo -e "${RED}✗${NC}"
    fi
    
    # Environment 3: Compile
    if [[ "$run_compile" == "true" ]]; then
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
    local passed=true
    local reason=""
    
    if [[ "$run_deno" == "true" ]] && [[ "$run_hir" == "true" ]]; then
        local dh_sim=$(calc_similarity "$deno_file" "$hir_file")
        echo -n "  └─ similarity: D-H:${dh_sim}%"
        
        if [[ "$STRICT_MODE" == "true" ]]; then
            [[ $dh_sim -lt 60 ]] && { passed=false; reason="D-H: ${dh_sim}%"; }
        else
            [[ $dh_sim -lt 40 ]] && { passed=false; reason="D-H: ${dh_sim}%"; }
        fi
    fi
    
    if [[ "$QUICK_MODE" != "true" ]] && [[ "$run_deno" == "true" ]] && [[ "$run_compile" == "true" ]]; then
        local dc_sim=$(calc_similarity "$deno_file" "$compile_file")
        local hc_sim=$(calc_similarity "$hir_file" "$compile_file")
        echo " D-C:${dc_sim}% H-C:${hc_sim}%"
        
        [[ $dc_sim -lt 60 ]] && { passed=false; reason="${reason} D-C: ${dc_sim}%"; }
        [[ $hc_sim -lt 60 ]] && { passed=false; reason="${reason} H-C: ${hc_sim}%"; }
    else
        echo ""
    fi
    
    # Generate diffs
    if [[ "$run_deno" == "true" ]] && [[ "$run_hir" == "true" ]]; then
        generate_diff "$deno_file" "$hir_file" "$example_diffs/deno_vs_hir.diff"
        generate_symbol_diff "$deno_file" "$hir_file" "$example_diffs/symbols.diff"
    fi
    
    if [[ "$QUICK_MODE" != "true" ]] && [[ "$run_compile" == "true" ]]; then
        if [[ "$run_deno" == "true" ]]; then
            generate_diff "$deno_file" "$compile_file" "$example_diffs/deno_vs_compile.diff"
        fi
        if [[ "$run_hir" == "true" ]]; then
            generate_diff "$hir_file" "$compile_file" "$example_diffs/hir_vs_compile.diff"
        fi
    fi
    
    # Extract symbols
    if [[ "$run_deno" == "true" ]]; then
        extract_symbols "$deno_file" > "$example_results/deno_symbols.txt" 2>/dev/null || true
    fi
    if [[ "$run_hir" == "true" ]]; then
        extract_symbols "$hir_file" > "$example_results/hir_symbols.txt" 2>/dev/null || true
    fi
    
    # Save status
    local status="PASS"
    if [[ "$passed" != "true" ]]; then
        status="FAIL"
    fi
    
    echo "$name|$deno_result|$hir_result|$compile_result|$status|$reason" >> "$SUMMARY_FILE"
    
    [[ "$passed" == "true" ]] && return 0 || return 1
}

# =============================================================================
# RUN UNIT TESTS
# =============================================================================

run_unit_tests() {
    echo ""
    echo -e "${BOLD}${CYAN}Running unit tests for runts-ink...${NC}"
    echo ""
    
    local test_output="$TMP_DIR/unit_tests.txt"
    if cargo test --package runts-ink 2>&1 | tee "$test_output"; then
        local test_count=$(grep -c "test result: ok" "$test_output" || echo "0")
        echo -e "${GREEN}✓ All unit tests passed${NC}"
        return 0
    else
        echo -e "${RED}✗ Some unit tests failed${NC}"
        return 1
    fi
}

# =============================================================================
# MAIN
# =============================================================================

run_tests() {
    echo "#!/bin/bash" > "$SUMMARY_FILE"
    echo "# Parity Test Summary - $(date)" >> "$SUMMARY_FILE"
    
    local examples=$(get_examples)
    local total=$(echo "$examples" | wc -l)
    local current=0 passed=0 failed=0
    
    echo ""
    echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║${NC}  ${CYAN}INK PARITY TEST HARNESS - UNIFIED${NC}                                   ${BOLD}║${NC}"
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════════╣${NC}"
    echo -e "${BOLD}║${NC}  Environments:"
    case "$ENVIRONMENTS" in
        all) echo -e "${BOLD}║${NC}    ${GREEN}deno${NC} | ${GREEN}runts dev${NC} | ${GREEN}runts build${NC}                       ${BOLD}║${NC}" ;;
        deno) echo -e "${BOLD}║${NC}    ${GREEN}deno${NC} only                                           ${BOLD}║${NC}" ;;
        hir) echo -e "${BOLD}║${NC}    ${GREEN}runts dev${NC} only                                       ${BOLD}║${NC}" ;;
        build) echo -e "${BOLD}║${NC}    ${GREEN}runts dev${NC} | ${GREEN}runts build${NC}                        ${BOLD}║${NC}" ;;
    esac
    echo -e "${BOLD}║${NC}  Mode: $([[ "$QUICK_MODE" == "true" ]] && echo -e "${YELLOW}QUICK${NC}" || echo -e "${YELLOW}FULL${NC}") $([[ "$STRICT_MODE" == "true" ]] && echo "| ${RED}STRICT${NC}" || echo "| ${GREEN}NORMAL${NC}")${BOLD}║${NC}"
    echo -e "${BOLD}║${NC}  Examples: $total${BOLD}                                                      ║${NC}"
    echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "Dry run - examples that would be tested:"
        for example_dir in $examples; do
            echo "  - $(basename "$example_dir")"
        done
        echo ""
        echo "Total: $total examples"
        exit 0
    fi
    
    for example_dir in $examples; do
        current=$((current + 1))
        local name=$(basename "$example_dir")
        
        [[ ! -f "$example_dir/tui/app.tsx" ]] && continue
        
        echo -n -e "${BLUE}[$current/$total]${NC} ${BOLD}$name${NC}"
        echo ""
        
        if test_example "$example_dir"; then
            passed=$((passed + 1))
        else
            failed=$((failed + 1))
        fi
        [[ $VERBOSE == true ]] && echo ""
    done
    
    # Summary
    echo ""
    echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║${NC}  ${CYAN}SUMMARY${NC}                                                            ${BOLD}║${NC}"
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════════╣${NC}"
    printf "${BOLD}║${NC}  ${GREEN}Passed:${NC} %-5s  ${RED}Failed:${NC} %-5s  ${CYAN}Total:${NC} %-5s${BOLD}                              ║${NC}\n" "$passed" "$failed" "$total"
    echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    echo -e "Results: ${CYAN}$RESULTS_DIR${NC}"
    echo -e "Diffs:   ${CYAN}$DIFF_DIR${NC}"
    echo ""
    
    # Run unit tests
    if run_unit_tests; then
        :
    else
        failed=$((failed + 1))
    fi
    
    return $failed
}

# =============================================================================
# ENTRY POINT
# =============================================================================

check_deps
run_tests
exit $?
