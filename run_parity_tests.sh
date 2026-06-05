#!/bin/bash
# =============================================================================
# INK PARITY TEST HARNESS - SIMPLE VERSION
# =============================================================================
# Tests look&feel parity across 3 environments:
#   1. deno        - Reference TypeScript runtime (npm:ink@7)
#   2. runts dev   - HIR runtime (QuickJS/HIR interpreter with hot-reload)
#   3. runts build - In-memory transpile + Rust compilation
#
# Usage: ./run_parity_tests.sh [OPTIONS]
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

EXAMPLES_DIR="$SCRIPT_DIR/examples"
RUNTS_BIN="$SCRIPT_DIR/target/debug/runts"
RUNTS_RELEASE_BIN="$SCRIPT_DIR/target/release/runts"

# Options
QUICK_MODE=false
SPECIFIC_EXAMPLES=""
STRICT_MODE=false
LIST_MODE=false
DRY_RUN=false
VERBOSE=false
KEEP_RESULTS=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --quick) QUICK_MODE=true; shift ;;
        --strict) STRICT_MODE=true; shift ;;
        --examples)
            shift
            while [[ $# -gt 0 ]] && [[ ! "$1" =~ ^-- ]]; do
                SPECIFIC_EXAMPLES="$SPECIFIC_EXAMPLES $1"
                shift
            done
            ;;
        --list) LIST_MODE=true; shift ;;
        --dry-run) DRY_RUN=true; shift ;;
        --verbose|-v) VERBOSE=true; shift ;;
        --keep) KEEP_RESULTS=true; shift ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo "  --quick       Skip compilation step (faster)"
            echo "  --strict      Treat known failures as actual failures"
            echo "  --examples N Specific examples to test"
            echo "  --list        List all examples"
            echo "  --dry-run     Show what would be tested"
            echo "  --keep        Keep temp files"
            exit 0
            ;;
        *) echo "Unknown: $1"; exit 1 ;;
    esac
done

# Check dependencies
check_deps() {
    if ! command -v deno &> /dev/null; then
        echo "ERROR: deno not found"
        exit 1
    fi
    
    if [[ ! -x "$RUNTS_BIN" ]] && [[ ! -x "$RUNTS_RELEASE_BIN" ]]; then
        echo "ERROR: runts not built"
        exit 1
    fi
    
    [[ -x "$RUNTS_BIN" ]] && echo "Using: $RUNTS_BIN" || echo "Using: $RUNTS_RELEASE_BIN"
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

# Calculate similarity
calc_similarity() {
    local file1="$1"
    local file2="$2"
    
    if [[ ! -f "$file1" ]] || [[ ! -f "$file2" ]]; then
        echo "0"
        return
    fi
    
    local norm1=$(normalize_output < "$file1" | sort -u)
    local norm2=$(normalize_output < "$file2" | sort -u)
    
    local lines1=$(echo "$norm1" | wc -l | tr -d ' ')
    local lines2=$(echo "$norm2" | wc -l | tr -d ' ')
    
    [[ "$lines1" -eq 0 ]] && [[ "$lines2" -eq 0 ]] && { echo "100"; return; }
    [[ "$lines1" -eq 0 ]] || [[ "$lines2" -eq 0 ]] && { echo "0"; return; }
    
    local matching=$(comm -12 <(echo "$norm1") <(echo "$norm2") 2>/dev/null | wc -l | tr -d ' ')
    local max_lines=$((lines1 > lines2 ? lines1 : lines2))
    [[ $max_lines -eq 0 ]] && max_lines=1
    
    echo $((matching * 100 / max_lines))
}

# Run deno
run_deno() {
    local example_dir="$1"
    local name
    name=$(basename "$example_dir")
    local output_file="$TMP_DIR/deno_$name.txt"
    local log_file="$TMP_DIR/deno_$name.log"
    
    timeout 5 deno run -A "$example_dir/main.tsx" > "$output_file" 2> "$log_file" || true
    
    normalize_output < "$output_file" > "$output_file.norm" 2>/dev/null
    mv "$output_file.norm" "$output_file"
    echo "$output_file"
}

# Run HIR
run_hir() {
    local example_dir="$1"
    local name
    name=$(basename "$example_dir")
    local output_file="$TMP_DIR/hir_$name.txt"
    local log_file="$TMP_DIR/hir_$name.log"
    
    local BIN="$RUNTS_BIN"
    [[ ! -x "$BIN" ]] && BIN="$RUNTS_RELEASE_BIN"
    
    timeout 10 "$BIN" hir-render "$example_dir/tui/app.tsx" > "$output_file" 2> "$log_file" || true
    
    # Remove DEBUG lines
    sed '/^DEBUG /d' "$output_file" > "$output_file.tmp" 2>/dev/null
    mv "$output_file.tmp" "$output_file"
    normalize_output < "$output_file" > "$output_file.norm" 2>/dev/null
    mv "$output_file.norm" "$output_file"
    echo "$output_file"
}

# Main
main() {
    echo "=============================================="
    echo "  INK PARITY TEST HARNESS"
    echo "=============================================="
    echo ""
    
    check_deps
    echo ""
    
    local examples
    examples=$(get_examples)
    local total
    total=$(echo "$examples" | wc -l | tr -d ' ')
    
    if [[ "$LIST_MODE" == "true" ]]; then
        echo "Available examples ($total):"
        for ex in $examples; do
            echo "  - $(basename "$ex")"
        done
        exit 0
    fi
    
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "Would test ($total examples):"
        for ex in $examples; do
            echo "  - $(basename "$ex")"
        done
        exit 0
    fi
    
    # Create temp directory
    TMP_DIR=$(mktemp -d "/tmp/runts_parity_XXXX")
    [[ "$KEEP_RESULTS" != "true" ]] && trap "rm -rf $TMP_DIR 2>/dev/null" EXIT
    
    echo "Testing $total examples..."
    echo ""
    
    local passed=0
    local failed=0
    
    for example_dir in $examples; do
        local name
        name=$(basename "$example_dir")
        
        echo -n "[$name] "
        
        local deno_file hir_file
        deno_file=$(run_deno "$example_dir")
        hir_file=$(run_hir "$example_dir")
        
        local sim
        sim=$(calc_similarity "$deno_file" "$hir_file")
        
        if [[ "$sim" -ge 60 ]]; then
            echo "✓ D-H:${sim}%"
            passed=$((passed + 1))
        else
            echo "✗ D-H:${sim}%"
            failed=$((failed + 1))
        fi
    done
    
    echo ""
    echo "=============================================="
    echo "  RESULTS: Passed=$passed Failed=$failed"
    echo "=============================================="
    
    [[ $failed -gt 0 ]] && exit 1 || exit 0
}

main "$@"
