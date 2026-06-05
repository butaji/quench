#!/bin/bash
# Ink Examples Parity Harness - 3 environments
# 1. deno - Reference TypeScript runtime  
# 2. runts hir-render - HIR runtime (pure Rust interpreter)
# 3. runts build --plugin ratatui - In-memory Rust compilation

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

EXAMPLES_DIR="./examples"
RUNTS_BIN="./target/release/runts"
TMP_DIR="/tmp/runts_ink_parity"
rm -rf "$TMP_DIR" 2>/dev/null
mkdir -p "$TMP_DIR"
trap "rm -rf $TMP_DIR" EXIT

# Use gtimeout on macOS, timeout on Linux
TIMEOUT="timeout"
if ! command -v timeout &> /dev/null; then
    TIMEOUT="gtimeout"
fi
if ! command -v gtimeout &> /dev/null; then
    TIMEOUT=""
fi

clean_debug() { sed '/^DEBUG /d' | sed '/^Error:/d' | grep -v "^info:" | grep -v "^warning:" | grep -v "^   Created" | grep -v "^   Compiling" | grep -v "^    Finished" | grep -v "^   Running" | grep -v "^Binary" | grep -v "^  Binary"; }

# Environment 1: Deno
run_deno() {
    local ed=$1
    cd "$ed"
    if [ -n "$TIMEOUT" ]; then
        ($TIMEOUT 3s deno run -A main.tsx > "$TMP_DIR/_d.txt" 2>&1) || true
    else
        (deno run -A main.tsx > "$TMP_DIR/_d.txt" 2>&1) &
        sleep 3
        kill %1 2>/dev/null || true
    fi
    cat "$TMP_DIR/_d.txt" 2>/dev/null | clean_debug | head -20 || echo "<DENO_ERR>"
    cd - > /dev/null
}

# Environment 2: runts hir-render (HIR runtime)
run_hir() {
    local app=$1/tui/app.tsx
    if [ -f "$app" ]; then
        $RUNTS_BIN hir-render "$app" 2>/dev/null | clean_debug | head -20 || echo "<HIR_ERR>"
    else
        echo "<NO_APP>"
    fi
}

# Find the binary - check multiple locations
find_binary() {
    local ed=$1
    local bin=""
    # Try release binary first
    for dir in "$ed/target/release" "$ed/.runts/build/target/release" "$ed/.runts/target/release"; do
        if [ -x "$dir/runts-app" ]; then
            bin="$dir/runts-app"
            break
        fi
    done
    # Try debug binary
    if [ -z "$bin" ]; then
        for dir in "$ed/target/debug" "$ed/.runts/build/target/debug" "$ed/.runts/target/debug"; do
            if [ -x "$dir/runts-app" ]; then
                bin="$dir/runts-app"
                break
            fi
        done
    fi
    echo "$bin"
}

# Environment 3: runts build --plugin ratatui (in-memory compilation)
run_compile() {
    local ed=$1
    local name=$(basename "$ed")
    
    # Clean any previous build artifacts
    rm -rf "$ed/.runts" 2>/dev/null || true
    rm -rf "$ed/target" 2>/dev/null || true
    
    # Build with ratatui plugin, keep build dir for inspection
    RUNTS_KEEP_BUILD=1 $RUNTS_BIN build "$ed" --plugin ratatui --release 2>&1 | clean_debug | tail -5 || true
    
    # Find the binary
    local bin=$(find_binary "$ed")
    
    if [ -n "$bin" ] && [ -x "$bin" ]; then
        if [ -n "$TIMEOUT" ]; then
            ($TIMEOUT 3s "$bin" > "$TMP_DIR/_c.txt" 2>&1) || true
        else
            ("$bin" > "$TMP_DIR/_c.txt" 2>&1) &
            sleep 3
            kill %1 2>/dev/null || true
        fi
        cat "$TMP_DIR/_c.txt" 2>/dev/null | clean_debug | head -20 || echo "<COMPILE_ERR>"
    else
        echo "<NO_BINARY>"
    fi
}

# Normalize output for comparison
normalize() { tr -d '\r' | sed 's/[[:space:]]*$//' | grep -v '^$' | tr -d ' \n\t'; }

echo "=============================================="
echo "INK PARITY TEST: deno vs hir vs compile"
echo "=============================================="

passed=0
failed=0
skipped=0
results=()

for ed in $(find "$EXAMPLES_DIR" -maxdepth 1 -type d -name "ink-*" | sort); do
    name=$(basename "$ed")
    
    if [ ! -f "$ed/tui/app.tsx" ]; then
        echo -e "${YELLOW}SKIP${NC} $name (no app.tsx)"
        skipped=$((skipped + 1))
        continue
    fi
    
    deno=$(run_deno "$ed")
    hir=$(run_hir "$ed")
    compile=$(run_compile "$ed")
    
    deno_n=$(normalize <<< "$deno")
    hir_n=$(normalize <<< "$hir")
    compile_n=$(normalize <<< "$compile")
    
    # Count passes
    d_h=0; d_c=0; h_c=0
    [ "$deno_n" = "$hir_n" ] && d_h=1
    [ "$deno_n" = "$compile_n" ] && d_c=1
    [ "$hir_n" = "$compile_n" ] && h_c=1
    
    all3=$((d_h + d_c + h_c))
    
    if [ "$all3" -eq 3 ]; then
        echo -e "${GREEN}✓${NC} $name (all 3 match)"
        passed=$((passed + 1))
    elif [ "$all3" -ge 2 ]; then
        echo -e "${YELLOW}⚠${NC} $name (2 of 3 match)"
        failed=$((failed + 1))
        results+=("$name")
    else
        echo -e "${RED}✗${NC} $name"
        failed=$((failed + 1))
        results+=("$name")
    fi
done

echo ""
echo "=============================================="
echo -e "Passed: ${GREEN}$passed${NC} | Failed: ${RED}$failed${NC} | Skipped: ${YELLOW}$skipped${NC}"

# Show detailed diffs for failed examples
if [ ${#results[@]} -gt 0 ]; then
    echo ""
    echo "=============================================="
    echo "DETAILED DIFFS"
    echo "=============================================="
    
    for name in "${results[@]}"; do
        ed="$EXAMPLES_DIR/$name"
        echo ""
        echo "--- $name ---"
        
        deno=$(run_deno "$ed")
        hir=$(run_hir "$ed")  
        compile=$(run_compile "$ed")
        
        echo "[DENO]"
        echo "$deno" | head -10 | sed 's/^/  /'
        
        echo "[HIR]"
        echo "$hir" | head -10 | sed 's/^/  /'
        
        echo "[COMPILE]"
        echo "$compile" | head -10 | sed 's/^/  /'
    done
fi

echo ""
[ $failed -eq 0 ] && echo "${GREEN}✓ ALL PARITY!${NC}" || echo "${RED}✗ Fix needed${NC}"
exit $failed
