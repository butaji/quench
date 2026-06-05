#!/bin/bash
# Ink Examples Parity Harness - 3 environments
# 1. deno - Reference TypeScript runtime  
# 2. runts hir-render - HIR runtime (pure Rust interpreter)
# 3. runts build --plugin ratatui - In-memory Rust compilation
#
# Usage: ./test_ink_parity.sh [--no-compile] [--examples ink-aligned ...]
#   --no-compile    Skip compile step (faster, for quick iteration)
#   --examples      List specific examples to test

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BLUE='\033[0;34m'
NC='\033[0m'
BOLD='\033[1m'

EXAMPLES_DIR="./examples"
RUNTS_BIN="./target/debug/runts"
TMP_DIR="/tmp/runts_ink_parity_$$"
RUN_COMPILE=true

# Parse args
SPECIFIC_EXAMPLES=""
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
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Cleanup on exit
cleanup() {
    rm -rf "$TMP_DIR" 2>/dev/null || true
}
trap cleanup EXIT

mkdir -p "$TMP_DIR"

# Check dependencies
check_deps() {
    local missing=()
    command -v deno >/dev/null 2>&1 || missing+=("deno")
    if [[ ! -x "$RUNTS_BIN" ]]; then
        missing+=("runts (not built)")
    fi
    if [[ ${#missing[@]} -gt 0 ]]; then
        echo "Missing dependencies: ${missing[*]}"
        echo "Build runts with: cargo build"
        exit 1
    fi
}

# Clean debug/build noise
clean_output() {
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
    grep -v "^help:" | \
    grep -v "^$" | \
    head -30
}

# Normalize output for comparison
normalize() {
    tr -d '\r' | \
    sed 's/[[:space:]]*$//' | \
    grep -v '^$' | \
    head -25
}

# Environment 1: Deno (static render only)
run_deno() {
    local ed=$1
    local name=$(basename "$ed")
    
    if [[ ! -f "$ed/main.tsx" ]]; then
        echo "<NO_MAIN>"
        return
    fi
    
    # Run deno with timeout
    timeout 5 deno run -A "$ed/main.tsx" > "$TMP_DIR/deno_$name.txt" 2>&1 &
    local pid=$!
    
    # Wait for up to 4 seconds
    local count=0
    while [[ $count -lt 4 ]] && kill -0 $pid 2>/dev/null; do
        sleep 1
        count=$((count + 1))
    done
    
    # Kill if still running
    if kill -0 $pid 2>/dev/null; then
        kill -9 $pid 2>/dev/null || true
    fi
    wait $pid 2>/dev/null || true
    
    # Check for errors
    if grep -q "Raw mode is not supported" "$TMP_DIR/deno_$name.txt" 2>/dev/null; then
        echo "<INTERACTIVE>"
        return
    fi
    
    if grep -q "error:" "$TMP_DIR/deno_$name.txt" 2>/dev/null; then
        # Check if it's a minor warning or real error
        if ! grep -q "TypeError\|ReferenceError\|SyntaxError" "$TMP_DIR/deno_$name.txt" 2>/dev/null; then
            echo "<DENO_WARN>"
            return
        fi
        echo "<DENO_ERR>"
        return
    fi
    
    cat "$TMP_DIR/deno_$name.txt" 2>/dev/null | clean_output || echo "<DENO_ERR>"
}

# Environment 2: runts hir-render (HIR runtime)
run_hir() {
    local app=$1/tui/app.tsx
    
    if [[ ! -f "$app" ]]; then
        echo "<NO_APP>"
        return
    fi
    
    timeout 5 $RUNTS_BIN hir-render "$app" 2>/dev/null | clean_output | head -25 || echo "<HIR_ERR>"
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
    
    RUNTS_KEEP_BUILD=1 timeout 60 $RUNTS_BIN build "$ed" --plugin ratatui --release > "$TMP_DIR/build_$name.txt" 2>&1
    
    if [[ $? -ne 0 ]]; then
        echo "<BUILD_ERR>"
        return
    fi
    
    # Find binary
    local bin=""
    for dir in "$ed/target/release" "$ed/.runts/target/release" "$ed/target/debug"; do
        if [[ -x "$dir/runts-app" ]]; then
            bin="$dir/runts-app"
            break
        fi
    done
    
    if [[ -z "$bin" ]] || [[ ! -x "$bin" ]]; then
        # Check if it's a simple non-interactive build
        echo "<NO_BINARY>"
        return
    fi
    
    # Run with timeout
    timeout 5 "$bin" > "$TMP_DIR/compile_$name.txt" 2>&1 &
    local pid=$!
    local count=0
    while [[ $count -lt 4 ]] && kill -0 $pid 2>/dev/null; do
        sleep 1
        count=$((count + 1))
    done
    if kill -0 $pid 2>/dev/null; then
        kill -9 $pid 2>/dev/null || true
    fi
    wait $pid 2>/dev/null || true
    
    cat "$TMP_DIR/compile_$name.txt" 2>/dev/null | clean_output | head -25 || echo "<RUN_ERR>"
}

# Compare two outputs and return match percentage
compare_outputs() {
    local file1=$1
    local file2=$2
    
    if [[ ! -f "$file1" ]] || [[ ! -f "$file2" ]]; then
        echo "0"
        return
    fi
    
    local lines1=$(normalize < "$file1" | wc -l)
    local lines2=$(normalize < "$file2" | wc -l)
    
    if [[ $lines1 -eq 0 ]] || [[ $lines2 -eq 0 ]]; then
        echo "0"
        return
    fi
    
    local matching=$(comm -12 <(normalize < "$file1" | sort -u) <(normalize < "$file2" | sort -u) 2>/dev/null | wc -l || echo 0)
    local min_lines=$((lines1 < lines2 ? lines1 : lines2))
    [[ $min_lines -eq 0 ]] && min_lines=1
    
    echo $((matching * 100 / min_lines))
}

# Get list of examples
get_examples() {
    if [[ -n "$SPECIFIC_EXAMPLES" ]]; then
        for ex in $SPECIFIC_EXAMPLES; do
            if [[ -d "$EXAMPLES_DIR/$ex" ]]; then
                echo "$EXAMPLES_DIR/$ex"
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

# Main
check_deps

echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║${NC}          ${CYAN}INK PARITY TEST: deno vs hir vs compile${NC}                                      ${BOLD}║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════════════════╝${NC}"
echo ""

passed=0
failed=0
skipped=0
interactive=0
errors=()

EXAMPLES=$(get_examples)
TOTAL=$(echo "$EXAMPLES" | wc -l)
CURRENT=0

for ed in $EXAMPLES; do
    CURRENT=$((CURRENT + 1))
    name=$(basename "$ed")
    
    if [[ ! -f "$ed/tui/app.tsx" ]]; then
        echo -e "${YELLOW}[$CURRENT/$TOTAL] SKIP${NC}  $name (no tui/app.tsx)"
        skipped=$((skipped + 1))
        continue
    fi
    
    echo -n -e "${BLUE}[$CURRENT/$TOTAL]${NC} Testing ${BOLD}$name${NC}... "
    
    # Run all three environments
    deno_out=$(run_deno "$ed")
    hir_out=$(run_hir "$ed")
    
    # Save outputs
    echo "$deno_out" > "$TMP_DIR/deno_$name.txt"
    echo "$hir_out" > "$TMP_DIR/hir_$name.txt"
    
    if [[ "$RUN_COMPILE" == "true" ]]; then
        compile_out=$(run_compile "$ed")
        echo "$compile_out" > "$TMP_DIR/compile_$name.txt"
    fi
    
    # Check for special cases
    if [[ "$deno_out" == "<INTERACTIVE>" ]] || [[ "$hir_out" == "<INTERACTIVE>" ]]; then
        echo -e "${YELLOW}INT${NC}"
        interactive=$((interactive + 1))
        continue
    fi
    
    if [[ "$deno_out" == "<NO_MAIN>" ]] || [[ "$hir_out" == "<NO_APP>" ]]; then
        echo -e "${YELLOW}SKIP${NC}"
        skipped=$((skipped + 1))
        continue
    fi
    
    # Calculate match percentages
    dh_match=$(compare_outputs "$TMP_DIR/deno_$name.txt" "$TMP_DIR/hir_$name.txt")
    
    if [[ "$RUN_COMPILE" == "true" ]]; then
        dc_match=$(compare_outputs "$TMP_DIR/deno_$name.txt" "$TMP_DIR/compile_$name.txt")
        hc_match=$(compare_outputs "$TMP_DIR/hir_$name.txt" "$TMP_DIR/compile_$name.txt")
        
        # Pass if at least 2 of 3 comparisons have >=70% match
        matches=0
        [[ $dh_match -ge 70 ]] && matches=$((matches + 1))
        [[ $dc_match -ge 70 ]] && matches=$((matches + 1))
        [[ $hc_match -ge 70 ]] && matches=$((matches + 1))
        
        if [[ $matches -ge 2 ]]; then
            echo -e "${GREEN}✓${NC} (DH:${dh_match}% DC:${dc_match}% HC:${hc_match}%)"
            passed=$((passed + 1))
        else
            echo -e "${RED}✗${NC} (DH:${dh_match}% DC:${dc_match}% HC:${hc_match}%)"
            failed=$((failed + 1))
            errors+=("$name")
        fi
    else
        # Just check deno-HIR match
        if [[ $dh_match -ge 70 ]]; then
            echo -e "${GREEN}✓${NC} (DH:${dh_match}%)"
            passed=$((passed + 1))
        else
            echo -e "${RED}✗${NC} (DH:${dh_match}%)"
            failed=$((failed + 1))
            errors+=("$name")
        fi
    fi
done

echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║${NC}                                  ${CYAN}SUMMARY${NC}                                            ${BOLD}║${NC}"
echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════════════════╣${NC}"
printf "${BOLD}║${NC}  ${GREEN}Passed:${NC}      %-5s" "$passed"
printf "  ${RED}Failed:${NC}      %-5s" "$failed"
printf "  ${YELLOW}Skipped:${NC}    %-5s" "$skipped"
printf "  ${YELLOW}Interactive:${NC} %-5s${BOLD}║${NC}\n" "$interactive"
echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════════════════╝${NC}"

# Show detailed diffs for failures
if [[ $failed -gt 0 ]]; then
    echo ""
    echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║${NC}                                  ${RED}FAILURES${NC}                                            ${BOLD}║${NC}"
    echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════════════════╝${NC}"
    
    for name in "${errors[@]}"; do
        echo ""
        echo -e "${RED}━━━ $name ━━━${NC}"
        
        echo -e "  ${CYAN}[DENO]${NC}"
        head -10 "$TMP_DIR/deno_$name.txt" 2>/dev/null | sed 's/^/    /' || echo "    <error>"
        
        echo -e "  ${CYAN}[HIR]${NC}"
        head -10 "$TMP_DIR/hir_$name.txt" 2>/dev/null | sed 's/^/    /' || echo "    <error>"
        
        if [[ "$RUN_COMPILE" == "true" ]]; then
            echo -e "  ${CYAN}[COMPILE]${NC}"
            head -10 "$TMP_DIR/compile_$name.txt" 2>/dev/null | sed 's/^/    /' || echo "    <error>"
        fi
    done
fi

echo ""
if [[ $failed -eq 0 ]]; then
    echo -e "${GREEN}${BOLD}╔══════════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}${BOLD}║${NC}                        ${GREEN}✓ ALL PARITY!${NC}                                          ${GREEN}${BOLD}║${NC}"
    echo -e "${GREEN}${BOLD}╚══════════════════════════════════════════════════════════════════════════════════╝${NC}"
    exit 0
else
    echo -e "${RED}${BOLD}╔══════════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${RED}${BOLD}║${NC}                        ${RED}✗ FIXES NEEDED${NC}                                        ${RED}${BOLD}║${NC}"
    echo -e "${RED}${BOLD}╚══════════════════════════════════════════════════════════════════════════════════╝${NC}"
    exit 1
fi
