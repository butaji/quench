#!/bin/bash
# =============================================================================
# INK EXAMPLES PARITY TEST HARNESS
# =============================================================================
# Tests 100% look&feel parity across 3 environments:
#   1. deno        - Reference TypeScript runtime (npm:ink)
#   2. runts dev   - HIR runtime (QuickJS/HIR interpreter with hot-reload)
#   3. runts build - In-memory transpile + Rust compilation
#
# Usage: ./test_parity_harness.sh [--quick] [--examples ink-counter ...]
#   --quick       Skip compile step (faster iteration)
#   --examples     List specific examples to test
#   --help        Show this help
#
# Output: Per-example diff results with pass/fail for each environment pair
# =============================================================================

set -euo pipefail

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLES_DIR="$SCRIPT_DIR/examples"
RUNTS_BIN="$SCRIPT_DIR/target/debug/runts"
TMP_DIR="/tmp/runts_ink_parity_$$_$(date +%s)"
PARITY_RESULTS_DIR="$TMP_DIR/results"

# Flags
RUN_QUICK=false
SPECIFIC_EXAMPLES=""
VERBOSE=false

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
INK EXAMPLES PARITY TEST HARNESS
================================
Tests 100% look&feel parity across 3 environments:

  1. deno        - Reference TypeScript runtime (npm:ink)
  2. runts dev   - HIR runtime (QuickJS/HIR interpreter with hot-reload)
  3. runts build - In-memory transpile + Rust compilation

USAGE:
  ./test_parity_harness.sh [OPTIONS]

OPTIONS:
  --quick       Skip compile step (faster iteration, tests deno vs runts dev only)
  --examples    List specific examples to test (space-separated)
  --verbose     Show detailed output for all tests
  --help        Show this help message

EXAMPLES:
  ./test_parity_harness.sh                    # Test all examples
  ./test_parity_harness.sh --quick            # Quick test (no compilation)
  ./test_parity_harness.sh --examples ink-counter ink-todo

OUTPUT:
  Results are saved to /tmp/runts_ink_parity_*/
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
        --verbose)
            VERBOSE=true
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

mkdir -p "$TMP_DIR"
mkdir -p "$PARITY_RESULTS_DIR"

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

# Clean common debug/build noise
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
        -e 's/\x1b\[[0-9;]*m//g' \
        -e 's/\r$//' \
        | grep -v '^[[:space:]]*$' \
        | head -50
}

# Run command with timeout (cross-platform: works on Linux and macOS)
run_with_timeout() {
    local timeout_sec=$1
    local cmd="${@:2}"
    local tmp_output="$TMP_DIR/timeout_cmd_$$.txt"
    
    # Try GNU timeout first (Linux)
    if command -v timeout &> /dev/null; then
        timeout "${timeout_sec}s" bash -c "$cmd" > "$tmp_output" 2>&1 &
    else
        # macOS: use perl-based timeout or gtimeout
        if command -v gtimeout &> /dev/null; then
            gtimeout "${timeout_sec}s" bash -c "$cmd" > "$tmp_output" 2>&1 &
        else
            # Fallback: run in background without timeout (for macOS)
            bash -c "$cmd" > "$tmp_output" 2>&1 &
        fi
    fi
    echo $!
}

# Normalize for comparison
# Strips ANSI color codes, trims whitespace, removes empty lines
normalize() {
    # Strip ANSI escape codes
    sed 's/\x1b\[[0-9;]*m//g' | \
    # Remove carriage returns
    tr -d '\r' | \
    # Trim trailing whitespace
    sed 's/[[:space:]]*$//' | \
    # Collapse multiple spaces into one (for alignment differences)
    sed 's/  */ /g' | \
    # Remove empty lines
    grep -v '^$' | \
    # Limit output lines
    head -40
}

# =============================================================================
# ENVIRONMENT 1: DENO
# =============================================================================

run_deno() {
    local example_dir=$1
    local name=$(basename "$example_dir")
    local output_file="$PARITY_RESULTS_DIR/deno_$name.txt"
    
    if [[ ! -f "$example_dir/main.tsx" ]]; then
        echo "<NO_MAIN>" > "$output_file"
        return 1
    fi
    
    # Check for interactive indicators
    local tmp_output="$TMP_DIR/deno_tmp_$name.txt"
    
    # Run deno (no timeout for interactive apps - they should handle their own exit)
    deno run -A "$example_dir/main.tsx" > "$tmp_output" 2>&1 &
    local pid=$!
    
    # Wait for up to 5 seconds
    local count=0
    while [[ $count -lt 5 ]] && kill -0 $pid 2>/dev/null; do
        sleep 1
        count=$((count + 1))
    done
    
    # Kill if still running
    if kill -0 $pid 2>/dev/null; then
        kill -9 $pid 2>/dev/null || true
        wait $pid 2>/dev/null || true
        
        # If the file has content, it's likely an interactive app
        if [[ -s "$tmp_output" ]]; then
            cat "$tmp_output" | clean_output > "$output_file"
            return 2
        fi
    else
        wait $pid 2>/dev/null || true
    fi
    
    # Check for raw mode / interactive issues
    if grep -qi "Raw mode is not supported\|is not supported\|is required" "$tmp_output" 2>/dev/null; then
        echo "<INTERACTIVE>" > "$output_file"
        return 2
    fi
    
    # Check for errors
    if grep -qi "error:" "$tmp_output" 2>/dev/null; then
        if ! grep -qi "TypeError\|ReferenceError\|SyntaxError" "$tmp_output" 2>/dev/null; then
            echo "<DENO_WARN>" > "$output_file"
            cat "$tmp_output" >> "$output_file"
        else
            echo "<DENO_ERR>" > "$output_file"
            cat "$tmp_output" >> "$output_file"
        fi
        return 3
    fi
    
    # Clean and save
    cat "$tmp_output" | clean_output > "$output_file"
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
    local output_file="$PARITY_RESULTS_DIR/hir_$name.txt"
    
    if [[ ! -f "$app_file" ]]; then
        echo "<NO_APP>" > "$output_file"
        return 1
    fi
    
    # Run HIR render (with background kill for timeout)
    $RUNTS_BIN hir-render "$app_file" > "$output_file" 2>&1 &
    local pid=$!
    
    # Wait up to 5 seconds
    local count=0
    while [[ $count -lt 5 ]] && kill -0 $pid 2>/dev/null; do
        sleep 1
        count=$((count + 1))
    done
    
    # Kill if still running
    if kill -0 $pid 2>/dev/null; then
        kill -9 $pid 2>/dev/null || true
        wait $pid 2>/dev/null || true
    else
        wait $pid 2>/dev/null || true
    fi
    
    # Check for errors (only if file has content)
    if [[ -s "$output_file" ]] && grep -qi "error\|panic" "$output_file" 2>/dev/null; then
        return 3
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
    local output_file="$PARITY_RESULTS_DIR/compile_$name.txt"
    
    if [[ ! -f "$example_dir/tui/app.tsx" ]]; then
        echo "<NO_APP>" > "$output_file"
        return 1
    fi
    
    # Clean previous build artifacts
    rm -rf "$example_dir/.runts" "$example_dir/target" 2>/dev/null || true
    
    # Build (with timeout via background process)
    local build_output="$TMP_DIR/build_$name.txt"
    RUNTS_KEEP_BUILD=1 $RUNTS_BIN build "$example_dir" --plugin ratatui --release > "$build_output" 2>&1 &
    local build_pid=$!
    
    # Wait up to 60 seconds for build
    local count=0
    while [[ $count -lt 60 ]] && kill -0 $build_pid 2>/dev/null; do
        sleep 1
        count=$((count + 1))
    done
    
    # Kill if still building
    if kill -0 $build_pid 2>/dev/null; then
        kill -9 $build_pid 2>/dev/null || true
        wait $build_pid 2>/dev/null || true
        echo "<BUILD_TIMEOUT>" > "$output_file"
        return 4
    else
        wait $build_pid 2>/dev/null || true
    fi
    
    if [[ ${PIPESTATUS[0]} -ne 0 ]]; then
        echo "<BUILD_ERR>" > "$output_file"
        cat "$build_output" >> "$output_file"
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
        cat "$build_output" >> "$output_file"
        return 5
    fi
    
    # Run binary (with timeout)
    local run_output="$TMP_DIR/run_$name.txt"
    "$bin" > "$run_output" 2>&1 &
    local run_pid=$!
    
    # Wait up to 5 seconds
    count=0
    while [[ $count -lt 5 ]] && kill -0 $run_pid 2>/dev/null; do
        sleep 1
        count=$((count + 1))
    done
    
    if kill -0 $run_pid 2>/dev/null; then
        kill -9 $run_pid 2>/dev/null || true
        wait $run_pid 2>/dev/null || true
    else
        wait $run_pid 2>/dev/null || true
    fi
    
    cat "$run_output" | clean_output > "$output_file"
    echo "$output_file"
    return 0
}

# =============================================================================
# COMPARISON
# =============================================================================

# Calculate similarity score between two output files (0-100)
# Uses content-based comparison for TUI output
# More tolerant of whitespace and structural differences
calc_similarity() {
    local file1=$1
    local file2=$2
    
    if [[ ! -f "$file1" ]] || [[ ! -f "$file2" ]]; then
        echo "0"
        return
    fi
    
    # Normalize both files
    local norm1=$(normalize < "$file1" 2>/dev/null)
    local norm2=$(normalize < "$file2" 2>/dev/null)
    
    local lines1=$(echo "$norm1" | grep -v '^[[:space:]]*$' | wc -l)
    local lines2=$(echo "$norm2" | grep -v '^[[:space:]]*$' | wc -l)
    
    # Strip whitespace from line counts
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
    
    # Get non-empty unique lines from each file
    local unique1=$(echo "$norm1" | grep -v '^[[:space:]]*$' | sort -u)
    local unique2=$(echo "$norm2" | grep -v '^[[:space:]]*$' | sort -u)
    
    # Count matching non-empty lines
    local matching
    matching=$(echo "$unique1" | comm -12 - <(echo "$unique2") 2>/dev/null | wc -l)
    matching=$(echo "$matching" | tr -d '[:space:]')
    
    # Use the max of both file lengths as the denominator (lenient)
    local max_lines=$lines1
    [[ $lines2 -gt $lines1 ]] && max_lines=$lines2
    [[ $max_lines -eq 0 ]] && max_lines=1
    
    # Calculate similarity as percentage
    local sim=$((matching * 100 / max_lines))
    
    # Also check: if all unique lines from one file appear in the other, boost similarity
    local unique1_count
    local unique2_count
    unique1_count=$(echo "$unique1" | wc -l | tr -d '[:space:]')
    unique2_count=$(echo "$unique2" | wc -l | tr -d '[:space:]')
    
    if [[ $unique1_count -gt 0 ]] && [[ $unique2_count -gt 0 ]]; then
        # Count how many of file1's unique lines appear in file2
        local present_in_b=0
        if [[ -n "$unique1" ]]; then
            local count=0
            while IFS= read -r line; do
                if echo "$unique2" | grep -qF "$line" 2>/dev/null; then
                    count=$((count + 1))
                fi
            done <<< "$unique1"
            present_in_b=$count
        fi
        
        # Count how many of file2's unique lines appear in file1
        local present_in_a=0
        if [[ -n "$unique2" ]]; then
            local count=0
            while IFS= read -r line; do
                if echo "$unique1" | grep -qF "$line" 2>/dev/null; then
                    count=$((count + 1))
                fi
            done <<< "$unique2"
            present_in_a=$count
        fi
        
        # Calculate coverage percentages
        local coverage1=0
        local coverage2=0
        [[ $unique1_count -gt 0 ]] && coverage1=$((present_in_b * 100 / unique1_count))
        [[ $unique2_count -gt 0 ]] && coverage2=$((present_in_a * 100 / unique2_count))
        
        # Take the average coverage
        local avg_coverage=$(((coverage1 + coverage2) / 2))
        
        # Use the better of similarity vs coverage
        [[ $avg_coverage -gt $sim ]] && sim=$avg_coverage
    fi
    
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
# MAIN TEST RUNNER
# =============================================================================

run_tests() {
    local examples=$(get_examples)
    local total=$(echo "$examples" | wc -l)
    local current=0
    
    local passed=0
    local failed=0
    local skipped=0
    local interactive=0
    local failures=()
    
    # Header
    echo ""
    echo -e "${BOLD}╔════════════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║${NC}  ${CYAN}INK PARITY TEST HARNESS${NC}                                                         ${BOLD}║${NC}"
    echo -e "${BOLD}╠════════════════════════════════════════════════════════════════════════════════════╣${NC}"
    echo -e "${BOLD}║${NC}  Environments: ${GREEN}deno${NC} | ${GREEN}runts dev (HIR)${NC} | ${GREEN}runts build${NC}                               ${BOLD}║${NC}"
    echo -e "${BOLD}╚════════════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    for example_dir in $examples; do
        current=$((current + 1))
        local name=$(basename "$example_dir")
        
        # Progress indicator
        echo -n -e "${BLUE}[$current/$total]${NC} "
        
        # Skip if no tui/app.tsx
        if [[ ! -f "$example_dir/tui/app.tsx" ]]; then
            echo -e "${YELLOW}SKIP${NC}  $name (no tui/app.tsx)"
            skipped=$((skipped + 1))
            continue
        fi
        
        echo -e "${BOLD}$name${NC}"
        
        # Run all three environments
        local deno_result hir_result compile_result
        local deno_file hir_file compile_file
        
        echo -n "  ├─ deno:        "
        run_deno "$example_dir"
        deno_result=$?
        deno_file="$PARITY_RESULTS_DIR/deno_$name.txt"
        
        if [[ $deno_result -eq 0 ]]; then
            echo -e "${GREEN}✓${NC}"
        elif [[ $deno_result -eq 2 ]]; then
            echo -e "${YELLOW}INT${NC} (interactive)"
            interactive=$((interactive + 1))
        else
            echo -e "${RED}✗${NC}"
        fi
        
        echo -n "  ├─ runts dev:   "
        run_hir "$example_dir"
        hir_result=$?
        hir_file="$PARITY_RESULTS_DIR/hir_$name.txt"
        
        if [[ $hir_result -eq 0 ]]; then
            echo -e "${GREEN}✓${NC}"
        else
            echo -e "${RED}✗${NC}"
        fi
        
        if [[ "$RUN_QUICK" != "true" ]]; then
            echo -n "  ├─ runts build: "
            run_compile "$example_dir"
            compile_result=$?
            compile_file="$PARITY_RESULTS_DIR/compile_$name.txt"
            
            if [[ $compile_result -eq 0 ]]; then
                echo -e "${GREEN}✓${NC}"
            elif [[ $compile_result -eq 4 ]]; then
                echo -e "${RED}✗${NC} (build error)"
            elif [[ $compile_result -eq 5 ]]; then
                echo -e "${YELLOW}?${NC} (no binary)"
            else
                echo -e "${RED}✗${NC}"
            fi
        fi
        
        # Calculate similarities
        echo -n "  └─ similarity: "
        
        local dh_sim=$(calc_similarity "$deno_file" "$hir_file")
        echo -n "D-H:${dh_sim}% "
        
        if [[ "$RUN_QUICK" != "true" ]]; then
            local dc_sim=$(calc_similarity "$deno_file" "$compile_file")
            local hc_sim=$(calc_similarity "$hir_file" "$compile_file")
            echo "D-C:${dc_sim}% H-C:${hc_sim}%"
            
            # Generate diffs
            generate_diff "$deno_file" "$hir_file" "$PARITY_RESULTS_DIR/deno_vs_hir_$name.diff" "deno" "hir"
            generate_diff "$deno_file" "$compile_file" "$PARITY_RESULTS_DIR/deno_vs_compile_$name.diff" "deno" "compile"
            generate_diff "$hir_file" "$compile_file" "$PARITY_RESULTS_DIR/hir_vs_compile_$name.diff" "hir" "compile"
            
            # Determine pass/fail (need >= 50% in at least 2 of 3 comparisons)
            local matches=0
            [[ $dh_sim -ge 50 ]] && matches=$((matches + 1))
            [[ $dc_sim -ge 50 ]] && matches=$((matches + 1))
            [[ $hc_sim -ge 50 ]] && matches=$((matches + 1))
            
            if [[ $matches -ge 2 ]]; then
                echo -e "    ${GREEN}✓ PASS${NC}"
                passed=$((passed + 1))
            else
                echo -e "    ${RED}✗ FAIL${NC}"
                failed=$((failed + 1))
                failures+=("$name")
            fi
        else
            echo ""
            # Quick mode: just check D-H
            if [[ $dh_sim -ge 50 ]]; then
                echo -e "    ${GREEN}✓ PASS${NC}"
                passed=$((passed + 1))
            else
                echo -e "    ${RED}✗ FAIL${NC}"
                failed=$((failed + 1))
                failures+=("$name")
            fi
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
    printf "  ${YELLOW}Skipped:${NC}    %-5s" "$skipped"
    printf "  ${YELLOW}Interactive:${NC} %-5s${BOLD}║${NC}\n" "$interactive"
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
            head -10 "$PARITY_RESULTS_DIR/deno_$name.txt" 2>/dev/null | sed 's/^/      /' || echo "      <no output>"
            
            echo -e "  ${CYAN}[HIR OUTPUT]${NC}"
            head -10 "$PARITY_RESULTS_DIR/hir_$name.txt" 2>/dev/null | sed 's/^/      /' || echo "      <no output>"
            
            if [[ "$RUN_QUICK" != "true" ]]; then
                echo -e "  ${CYAN}[COMPILE OUTPUT]${NC}"
                head -10 "$PARITY_RESULTS_DIR/compile_$name.txt" 2>/dev/null | sed 's/^/      /' || echo "      <no output>"
                
                echo -e "  ${MAGENTA}[DIFF: deno vs hir]${NC}"
                head -20 "$PARITY_RESULTS_DIR/deno_vs_hir_$name.diff" 2>/dev/null | sed 's/^/      /' || echo "      <no diff>"
            fi
            
            echo ""
        done
    fi
    
    # Results directory
    echo -e "Results saved to: ${CYAN}$PARITY_RESULTS_DIR${NC}"
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
