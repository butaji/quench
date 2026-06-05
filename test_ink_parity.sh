#!/bin/bash
# Ink Examples Parity Harness - Fast test: Deno vs HIR
set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

EXAMPLES_DIR="./examples"
RUNTS_BIN="./target/release/runts"
TMP_DIR="/tmp/runts_ink_parity"

rm -rf "$TMP_DIR" 2>/dev/null
mkdir -p "$TMP_DIR"
trap "rm -rf $TMP_DIR" EXIT

clean_debug() { sed '/^DEBUG /d' | sed '/^eprintln/d' | sed '/^Error:/d'; }

run_deno() {
    local ed=$1
    local out_file="$TMP_DIR/_deno_$$.txt"
    cd "$ed"
    (deno run -A main.tsx > "$out_file" 2>&1) &
    local pid=$!
    sleep 3
    kill $pid 2>/dev/null || true
    wait $pid 2>/dev/null || true
    if [ -f "$out_file" ]; then
        cat "$out_file" | clean_debug
        rm "$out_file"
    else
        echo "<DENO_ERR>"
    fi
    cd - > /dev/null
}

run_hir() {
    local app=$1/tui/app.tsx
    if [ -f "$app" ]; then
        $RUNTS_BIN hir-render "$app" 2>/dev/null | clean_debug || echo "<HIR_ERR>"
    else
        echo "<NO_APP>"
    fi
}

normalize() { tr -d '\r' | sed 's/[[:space:]]*$//' | grep -v '^$' | tr -d ' \n\t'; }

echo "=============================================="
echo "INK PARITY TEST: deno vs runts hir-render"
echo "=============================================="

passed=0
failed=0
skipped=0

for ed in $(find "$EXAMPLES_DIR" -maxdepth 1 -type d -name "ink-*" | sort); do
    name=$(basename "$ed")
    
    if [ ! -f "$ed/tui/app.tsx" ]; then
        echo -e "${YELLOW}SKIP${NC} $name"
        skipped=$((skipped + 1))
        continue
    fi
    
    deno=$(run_deno "$ed")
    hir=$(run_hir "$ed")
    
    if [ "$hir" = "<NO_APP>" ]; then
        echo -e "${YELLOW}SKIP${NC} $name"
        skipped=$((skipped + 1))
        continue
    fi
    
    deno_n=$(normalize <<< "$deno")
    hir_n=$(normalize <<< "$hir")
    
    if [ "$deno_n" = "$hir_n" ]; then
        echo -e "${GREEN}✓${NC} $name"
        passed=$((passed + 1))
    else
        echo -e "${RED}✗${NC} $name"
        failed=$((failed + 1))
        echo "  [DENO]"
        echo "$deno" | head -8 | sed 's/^/    /'
        echo "  [HIR]"
        echo "$hir" | head -8 | sed 's/^/    /'
    fi
done

echo ""
echo "=============================================="
echo -e "Passed: ${GREEN}$passed${NC} | Failed: ${RED}$failed${NC} | Skipped: ${YELLOW}$skipped${NC}"

[ $failed -eq 0 ] && echo "${GREEN}✓ ALL PARITY!${NC}" || echo "${RED}✗ Fix needed${NC}"
exit $failed
