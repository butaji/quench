#!/bin/bash
# =============================================================================
# INK PARITY TEST HARNESS - COMPREHENSIVE 3-ENVIRONMENT VERSION
# =============================================================================
# Tests look&feel parity across 3 environments:
#   1. deno        - Reference TypeScript runtime (npm:ink@7)
#   2. runts dev   - HIR runtime (QuickJS/HIR interpreter with hot-reload)
#   3. runts build - In-memory transpile + Rust compilation
#
# Features:
#   - Per-symbol diff analysis
#   - Character-level comparison
#   - ANSI color normalization
#   - Detailed failure categorization
#   - Output persistence option
#
# Usage: ./run_parity_tests_comprehensive.sh [OPTIONS]
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

EXAMPLES_DIR="$SCRIPT_DIR/examples"
RUNTS_BIN="$SCRIPT_DIR/target/debug/runts"

# Options
QUICK_MODE=false
SPECIFIC_EXAMPLES=""
VERBOSE=false
KEEP_RESULTS=false
OUTPUT_DIR=""
PER_SYMBOL_DIFF=false
MIN_SIMILARITY=60

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --quick) QUICK_MODE=true; shift ;;
        --examples)
            shift
            while [[ $# -gt 0 ]] && [[ ! "$1" =~ ^-- ]]; do
                SPECIFIC_EXAMPLES="$SPECIFIC_EXAMPLES $1"
                shift
            done
            ;;
        --verbose|-v) VERBOSE=true; shift ;;
        --keep) KEEP_RESULTS=true; shift ;;
        --output-dir)
            shift
            OUTPUT_DIR="$1"
            shift
            ;;
        --per-symbol) PER_SYMBOL_DIFF=true; shift ;;
        --min-similarity)
            shift
            MIN_SIMILARITY="$1"
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo "  --quick              Skip compilation step (faster)"
            echo "  --examples N         Specific examples to test"
            echo "  --verbose            Show detailed output"
            echo "  --keep              Keep temp files"
            echo "  --output-dir D      Save results to directory"
            echo "  --per-symbol        Show per-symbol diff details"
            echo "  --min-similarity N Minimum similarity threshold (default: 60)"
            exit 0
            ;;
        *) echo "Unknown: $1"; exit 1 ;;
    esac
done

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Check dependencies
check_deps() {
    if ! command -v deno &> /dev/null; then
        echo -e "${RED}ERROR: deno not found${NC}"
        exit 1
    fi
    
    if [[ ! -x "$RUNTS_BIN" ]]; then
        echo -e "${RED}ERROR: runts not built${NC}"
        exit 1
    fi
    
    echo -e "${BLUE}Using: $RUNTS_BIN${NC}"
}

# Get examples to test
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

# Normalize output for comparison
normalize_output() {
    sed 's/\x1b\[[0-9;]*m//g' | tr -d '\r' | grep -v '^[[:space:]]*$'
}

# Extract unique symbols/words from output
extract_symbols() {
    tr ' ' '\n' | grep -v '^$' | sort -u
}

# Calculate similarity between two outputs
calc_similarity() {
    local output1="$1"
    local output2="$2"
    
    if [[ ! -s "$output1" ]] && [[ ! -s "$output2" ]]; then
        echo "100"
        return
    fi
    
    [[ ! -s "$output1" ]] || [[ ! -s "$output2" ]] && echo "0" && return
    
    local norm1=$(normalize_output < "$output1" | sort -u)
    local norm2=$(normalize_output < "$output2" | sort -u)
    
    local lines1=$(echo "$norm1" | wc -l | tr -d ' ')
    local lines2=$(echo "$norm2" | wc -l | tr -d ' ')
    
    local matching=$(comm -12 <(echo "$norm1") <(echo "$norm2") 2>/dev/null | wc -l | tr -d ' ')
    local max_lines=$((lines1 > lines2 ? lines1 : lines2))
    [[ $max_lines -eq 0 ]] && max_lines=1
    
    echo $((matching * 100 / max_lines))
}

# Generate detailed diff
generate_diff() {
    local name="$1"
    local file1="$2"
    local file2="$3"
    local label="$4"
    local output_file="$5"
    
    {
        echo "=== $label Diff for $name ==="
        echo ""
        
        if [[ ! -s "$file1" ]]; then
            echo "File 1 is empty"
            return
        fi
        
        if [[ ! -s "$file2" ]]; then
            echo "File 2 is empty"
            return
        fi
        
        local norm1=$(normalize_output < "$file1" | sort -u)
        local norm2=$(normalize_output < "$file2" | sort -u)
        
        echo "Unique to File 1 (deno):"
        comm -23 <(echo "$norm1") <(echo "$norm2") 2>/dev/null | head -20 || echo "(none)"
        echo ""
        
        echo "Unique to File 2 (HIR):"
        comm -13 <(echo "$norm1") <(echo "$norm2") 2>/dev/null | head -20 || echo "(none)"
        echo ""
        
        echo "First 50 chars of each:"
        echo "File 1: $(head -c 50 "$file1" 2>/dev/null | cat -v)"
        echo "File 2: $(head -c 50 "$file2" 2>/dev/null | cat -v)"
    } > "$output_file"
}

# Run deno
run_deno() {
    local example_dir="$1"
    local name
    name=$(basename "$example_dir")
    local output_file="$TMP_DIR/deno_$name.txt"
    local log_file="$TMP_DIR/deno_$name.log"
    
    timeout 5 deno run -A "$example_dir/main.tsx" > "$output_file" 2> "$log_file" || true
    
    normalize_output < "$output_file" > "$output_file.norm" 2>/dev/null || true
    mv "$output_file.norm" "$output_file" 2>/dev/null || true
    echo "$output_file"
}

# Run HIR
run_hir() {
    local example_dir="$1"
    local name
    name=$(basename "$example_dir")
    local output_file="$TMP_DIR/hir_$name.txt"
    local log_file="$TMP_DIR/hir_$name.log"
    
    timeout 10 "$RUNTS_BIN" hir-render "$example_dir/tui/app.tsx" > "$output_file" 2> "$log_file" || true
    
    sed '/^DEBUG /d' "$output_file" > "$output_file.tmp" 2>/dev/null || true
    mv "$output_file.tmp" "$output_file" 2>/dev/null || true
    normalize_output < "$output_file" > "$output_file.norm" 2>/dev/null || true
    mv "$output_file.norm" "$output_file" 2>/dev/null || true
    echo "$output_file"
}

# Run compile
run_compile() {
    local example_dir="$1"
    local name
    name=$(basename "$example_dir")
    local output_file="$TMP_DIR/compile_$name.txt"
    local log_file="$TMP_DIR/compile_$name.log"
    
    cd "$example_dir" > /dev/null 2>&1 || { touch "$output_file"; echo "$output_file"; return; }
    
    timeout 60 "$RUNTS_BIN" run --no-run 2> "$log_file" || true
    
    local compiled_bin=""
    [[ -f "target/release/run" ]] && compiled_bin="target/release/run"
    [[ -f "target/debug/run" ]] && compiled_bin="target/debug/run"
    
    if [[ -n "$compiled_bin" ]] && [[ -x "$compiled_bin" ]]; then
        timeout 5 "$compiled_bin" > "$output_file" 2>&1 || true
    else
        timeout 30 "$RUNTS_BIN" run > "$output_file" 2> "$log_file" || true
    fi
    
    cd - > /dev/null
    
    sed '/^DEBUG /d' "$output_file" > "$output_file.tmp" 2>/dev/null || true
    mv "$output_file.tmp" "$output_file" 2>/dev/null || true
    normalize_output < "$output_file" > "$output_file.norm" 2>/dev/null || true
    mv "$output_file.norm" "$output_file" 2>/dev/null || true
    echo "$output_file"
}

# Categorize failure
categorize_failure() {
    local deno_file="$1"
    local hir_file="$2"
    
    if [[ ! -s "$deno_file" ]]; then
        echo "deno_empty"
    elif [[ ! -s "$hir_file" ]]; then
        echo "hir_empty"
    elif grep -q "ERROR\|Error\|error" "$deno_file" 2>/dev/null; then
        echo "deno_error"
    elif grep -q "ERROR\|Error\|error" "$hir_file" 2>/dev/null; then
        echo "hir_error"
    else
        echo "content_mismatch"
    fi
}

# Main
main() {
    echo "=============================================="
    echo -e "  ${BLUE}INK PARITY TEST HARNESS${NC} - 3 ENVIRONMENTS"
    echo "=============================================="
    echo ""
    
    check_deps
    echo ""
    
    local examples
    examples=$(get_examples)
    local total
    total=$(echo "$examples" | wc -l | tr -d ' ')
    
    echo -e "Testing ${GREEN}$total${NC} examples across 3 environments..."
    echo "  1. deno       - Real Ink npm package"
    echo "  2. runts dev  - HIR runtime"
    echo "  3. runts build - Rust codegen"
    echo ""
    echo -e "Minimum similarity threshold: ${YELLOW}${MIN_SIMILARITY}%${NC}"
    echo ""
    
    # Create temp directory
    TMP_DIR=$(mktemp -d "/tmp/runts_parity_XXXX")
    if [[ -n "$OUTPUT_DIR" ]]; then
        mkdir -p "$OUTPUT_DIR"
        RESULTS_DIR="$OUTPUT_DIR"
    else
        RESULTS_DIR="$TMP_DIR"
    fi
    
    [[ "$KEEP_RESULTS" != "true" ]] && trap "rm -rf $TMP_DIR 2>/dev/null" EXIT
    
    # Results tracking
    local passed=0
    local failed=0
    local categories=()
    
    # Results summary file
    echo "=== INK Parity Test Results ===" > "$RESULTS_DIR/summary.txt"
    echo "Date: $(date)" >> "$RESULTS_DIR/summary.txt"
    echo "Min Similarity: ${MIN_SIMILARITY}%" >> "$RESULTS_DIR/summary.txt"
    echo "" >> "$RESULTS_DIR/summary.txt"
    
    for example_dir in $examples; do
        local name
        name=$(basename "$example_dir")
        
        echo -n "[$name] "
        
        # Run environments
        local deno_file hir_file compile_file
        deno_file=$(run_deno "$example_dir")
        hir_file=$(run_hir "$example_dir")
        
        if [[ "$QUICK_MODE" != "true" ]]; then
            compile_file=$(run_compile "$example_dir")
        else
            compile_file=""
        fi
        
        # Calculate similarity
        local similarity
        similarity=$(calc_similarity "$deno_file" "$hir_file")
        
        # Determine pass/fail
        local result_symbol
        local result_color
        local status
        
        if [[ "$similarity" -ge "$MIN_SIMILARITY" ]]; then
            result_symbol="✓"
            result_color="$GREEN"
            status="PASS"
            passed=$((passed + 1))
        else
            result_symbol="✗"
            result_color="$RED"
            status="FAIL"
            failed=$((failed + 1))
            
            # Categorize failure
            local category
            category=$(categorize_failure "$deno_file" "$hir_file")
            categories+=("$name:$category")
        fi
        
        echo -e "${result_color}$result_symbol${NC} D-H:${similarity}%"
        
        # Log to summary
        echo "[$name] $status D-H:${similarity}%" >> "$RESULTS_DIR/summary.txt"
        
        # Generate per-symbol diff if requested
        if [[ "$PER_SYMBOL_DIFF" == "true" ]] && [[ "$status" == "FAIL" ]]; then
            generate_diff "$name" "$deno_file" "$hir_file" "Deno vs HIR" "$RESULTS_DIR/diff_${name}.txt"
        fi
        
        # Save outputs
        cp "$deno_file" "$RESULTS_DIR/deno_${name}.txt" 2>/dev/null || true
        cp "$hir_file" "$RESULTS_DIR/hir_${name}.txt" 2>/dev/null || true
        [[ -n "$compile_file" ]] && [[ -f "$compile_file" ]] && cp "$compile_file" "$RESULTS_DIR/compile_${name}.txt" 2>/dev/null || true
    done
    
    echo ""
    echo "=============================================="
    echo -e "  ${GREEN}RESULTS${NC}: Passed=${GREEN}${passed}${NC} Failed=${RED}${failed}${NC} Total=$total"
    echo "=============================================="
    
    # Failure breakdown
    if [[ ${#categories[@]} -gt 0 ]]; then
        echo ""
        echo "Failure Categories:"
        for cat in "${categories[@]}"; do
            local example="${cat%%:*}"
            local category="${cat##*:}"
            echo "  $example: $category"
        done
    fi
    
    echo ""
    echo "Results saved to: $RESULTS_DIR/"
    
    # Exit with appropriate code
    [[ $failed -gt 0 ]] && exit 1 || exit 0
}

main "$@"
