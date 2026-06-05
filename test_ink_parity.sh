#!/bin/bash
# Ink Examples Parity Harness - 3 environments
# 1. deno - Reference TypeScript runtime  
# 2. runts hir-render - HIR runtime (pure Rust interpreter)
# 3. runts build --plugin ratatui - In-memory Rust compilation
#
# Usage: ./test_ink_parity.sh [--compile] [--examples ink-aligned ink-border-color ...]
#   --compile    Skip compile step (faster, for quick iteration)
#   --examples   List specific examples to test

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BLUE='\033[0;34m'
NC='\033[0m'

EXAMPLES_DIR="./examples"
RUNTS_BIN="./target/debug/runts"
TMP_DIR="/tmp/runts_ink_parity"
RUN_COMPILE=true

# Parse args
while [[ $# -gt 0 ]]; do
    case $1 in
        --no-compile)
            RUN_COMPILE=false
            shift
            ;;
        --examples)
            shift
            SPECIFIC_EXAMPLES="$*"
            break
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

rm -rf "$TMP_DIR" 2>/dev/null
mkdir -p "$TMP_DIR"
trap "rm -rf $TMP_DIR" EXIT

# Clean debug output
clean_debug() {
    sed '/^DEBUG /d' | \
    grep -v "^info:" | \
    grep -v "^warning:" | \
    grep -v "^   Created" | \
    grep -v "^   Compiling" | \
    grep -v "^    Finished" | \
    grep -v "^   Running" | \
    grep -v "^Binary" | \
    grep -v "^  Binary" | \
    grep -v "^2026-" | \
    grep -v "^thread " | \
    grep -v "^note:" | \
    grep -v "^   ---" | \
    grep -v "^help:"
}

# Environment 1: Deno (static render only)
run_deno() {
    local ed=$1
    local name=$(basename "$ed")
    
    if [[ ! -f "$ed/main.tsx" ]]; then
        echo "<NO_MAIN>"
        return
    fi
    
    # Run deno and capture output
    deno run -A "$ed/main.tsx" > "$TMP_DIR/deno_$name.txt" 2>&1 &
    local pid=$!
    
    # Wait for up to 4 seconds
    local count=0
    while [[ $count -lt 4 ]] && kill -0 $pid 2>/dev/null; do
        sleep 1
        count=$((count + 1))
    done
    
    # Kill if still running
    if kill -0 $pid 2>/dev/null; then
        kill $pid 2>/dev/null || true
    fi
    wait $pid 2>/dev/null || true
    
    if grep -q "Raw mode is not supported" "$TMP_DIR/deno_$name.txt" 2>/dev/null; then
        echo "<INTERACTIVE>"
        return
    fi
    
    if grep -q "error:" "$TMP_DIR/deno_$name.txt" 2>/dev/null; then
        echo "<DENO_ERR>"
        return
    fi
    
    cat "$TMP_DIR/deno_$name.txt" 2>/dev/null | clean_debug | head -25 || echo "<DENO_ERR>"
}

# Environment 2: runts hir-render (HIR runtime)
run_hir() {
    local app=$1/tui/app.tsx
    
    if [[ ! -f "$app" ]]; then
        echo "<NO_APP>"
        return
    fi
    
    $RUNTS_BIN hir-render "$app" 2>/dev/null | clean_debug | head -25 || echo "<HIR_ERR>"
}

# Environment 3: runts build --plugin ratatui
run_compile() {
    local ed=$1
    local name=$(basename "$ed")
    
    if [[ ! -f "$ed/tui/app.tsx" ]]; then
        echo "<NO_APP>"
        return
    fi
    
    # Clean and build
    rm -rf "$ed/.runts" "$ed/target" 2>/dev/null || true
    
    RUNTS_KEEP_BUILD=1 $RUNTS_BIN build "$ed" --plugin ratatui --release > "$TMP_DIR/build_$name.txt" 2>&1
    
    # Find binary
    local bin=""
    for dir in "$ed/target/release" "$ed/.runts/target/release"; do
        if [[ -x "$dir/runts-app" ]]; then
            bin="$dir/runts-app"
            break
        fi
    done
    
    if [[ -z "$bin" ]] || [[ ! -x "$bin" ]]; then
        echo "<NO_BINARY>"
        return
    fi
    
    # Run with timeout
    "$bin" > "$TMP_DIR/compile_$name.txt" 2>&1 &
    local pid=$!
    local count=0
    while [[ $count -lt 4 ]] && kill -0 $pid 2>/dev/null; do
        sleep 1
        count=$((count + 1))
    done
    if kill -0 $pid 2>/dev/null; then
        kill $pid 2>/dev/null || true
    fi
    wait $pid 2>/dev/null || true
    
    cat "$TMP_DIR/compile_$name.txt" 2>/dev/null | clean_debug | head -25 || echo "<RUN_ERR>"
}

# Normalize output for comparison
normalize() {
    tr -d '\r' | \
    sed 's/[[:space:]]*$//' | \
    grep -v '^$' | \
    head -20 | \
    tr -d ' \t\n'
}

# Count matching lines (visual similarity)
count_matches() {
    local file1=$1
    local file2=$2
    
    if [[ ! -f "$file1" ]] || [[ ! -f "$file2" ]]; then
        echo "0"
        return
    fi
    
    local count=$(comm -12 <(sort "$file1" 2>/dev/null | uniq) <(sort "$file2" 2>/dev/null | uniq) 2>/dev/null | wc -l || echo "0")
    echo "$count"
}

# Get list of examples
get_examples() {
    if [[ -n "${SPECIFIC_EXAMPLES:-}" ]]; then
        echo "$SPECIFIC_EXAMPLES"
    else
        for dir in "$EXAMPLES_DIR"/ink-*; do
            if [[ -d "$dir" ]]; then
                basename "$dir"
            fi
        done | sort
    fi
}

echo ""
echo "╔══════════════════════════════════════════════════════════════════════════════╗"
echo "║         INK PARITY TEST: deno vs hir vs compile                              ║"
echo "╚══════════════════════════════════════════════════════════════════════════════╝"
echo ""

passed=0
failed=0
skipped=0
interactive=0
details=()

# Find all examples
EXAMPLES=$(get_examples)
TOTAL=$(echo "$EXAMPLES" | wc -l)
CURRENT=0

for ed in $EXAMPLES; do
    CURRENT=$((CURRENT + 1))
    full_path="$EXAMPLES_DIR/$ed"
    
    if [[ ! -f "$full_path/tui/app.tsx" ]]; then
        echo -e "${YELLOW}[$CURRENT/$TOTAL] SKIP${NC}  $ed (no tui/app.tsx)"
        skipped=$((skipped + 1))
        continue
    fi
    
    echo -n -e "${BLUE}[$CURRENT/$TOTAL]${NC} Testing $ed... "
    
    # Run all three environments
    deno_out=$(run_deno "$full_path")
    hir_out=$(run_hir "$full_path")
    
    if [[ "$deno_out" == "<INTERACTIVE>" ]]; then
        echo -e "${YELLOW}INT${NC}"
        interactive=$((interactive + 1))
        continue
    fi
    
    if [[ "$deno_out" == "<NO_MAIN>" ]]; then
        echo -e "${YELLOW}SKIP${NC} (no main.tsx)"
        skipped=$((skipped + 1))
        continue
    fi
    
    # Save outputs for comparison
    echo "$deno_out" > "$TMP_DIR/deno_$ed.txt"
    echo "$hir_out" > "$TMP_DIR/hir_$ed.txt"
    
    if [[ "$RUN_COMPILE" == "true" ]]; then
        compile_out=$(run_compile "$full_path")
        echo "$compile_out" > "$TMP_DIR/compile_$ed.txt"
    fi
    
    # Normalize and compare
    deno_n=$(normalize < "$TMP_DIR/deno_$ed.txt")
    hir_n=$(normalize < "$TMP_DIR/hir_$ed.txt")
    
    if [[ "$RUN_COMPILE" == "true" ]]; then
        compile_n=$(normalize < "$TMP_DIR/compile_$ed.txt")
        
        d_h=$([[ "$deno_n" == "$hir_n" ]] && echo 1 || echo 0)
        d_c=$([[ "$deno_n" == "$compile_n" ]] && echo 1 || echo 0)
        h_c=$([[ "$hir_n" == "$compile_n" ]] && echo 1 || echo 0)
        matches=$((d_h + d_c + h_c))
    else
        d_h=$([[ "$deno_n" == "$hir_n" ]] && echo 1 || echo 0)
        matches=$d_h
    fi
    
    if [[ "$matches" -ge 2 ]]; then
        echo -e "${GREEN}✓${NC}"
        passed=$((passed + 1))
    else
        echo -e "${RED}✗${NC}"
        failed=$((failed + 1))
        details+=("$ed")
    fi
done

echo ""
echo "╔══════════════════════════════════════════════════════════════════════════════╗"
echo "║                              SUMMARY                                        ║"
echo "╠══════════════════════════════════════════════════════════════════════════════╣"
echo -e "║  ${GREEN}Passed:${NC}      $passed"
echo -e "║  ${RED}Failed:${NC}      $failed"
echo -e "║  ${YELLOW}Skipped:${NC}    $skipped"
echo -e "║  ${YELLOW}Interactive:${NC} $interactive"
echo "╚══════════════════════════════════════════════════════════════════════════════╝"

# Show detailed diffs for failures
if [[ $failed -gt 0 ]]; then
    echo ""
    echo "╔══════════════════════════════════════════════════════════════════════════════╗"
    echo "║                              FAILURES                                      ║"
    echo "╚══════════════════════════════════════════════════════════════════════════════╝"
    
    for name in "${details[@]}"; do
        echo ""
        echo -e "${RED}━━━ $name ━━━${NC}"
        
        echo -e "${CYAN}[DENO]${NC}"
        head -15 "$TMP_DIR/deno_$name.txt" 2>/dev/null | sed 's/^/  /' || echo "  <error>"
        
        echo -e "${CYAN}[HIR]${NC}"
        head -15 "$TMP_DIR/hir_$name.txt" 2>/dev/null | sed 's/^/  /' || echo "  <error>"
        
        if [[ "$RUN_COMPILE" == "true" ]]; then
            echo -e "${CYAN}[COMPILE]${NC}"
            head -15 "$TMP_DIR/compile_$name.txt" 2>/dev/null | sed 's/^/  /' || echo "  <error>"
        fi
        
        # Show key differences
        echo ""
        echo "  Key diff lines:"
        diff "$TMP_DIR/deno_$name.txt" "$TMP_DIR/hir_$name.txt" 2>/dev/null | head -10 | sed 's/^/    /' || echo "    (no line diff available)"
    done
fi

echo ""
if [[ $failed -eq 0 ]]; then
    echo -e "${GREEN}╔══════════════════════════════════════════════════════════════════════════════╗"
    echo -e "║                        ✓ ALL PARITY!                                      ║"
    echo -e "╚══════════════════════════════════════════════════════════════════════════════╝${NC}"
    exit 0
else
    echo -e "${RED}╔══════════════════════════════════════════════════════════════════════════════╗"
    echo -e "║                        ✗ FIXES NEEDED                                      ║"
    echo -e "╚══════════════════════════════════════════════════════════════════════════════╝${NC}"
    exit 1
fi
