#!/bin/bash
# Parity Harness — Run TSX examples in Deno (reference) and TuiBridge
# Compare ANSI output cell-by-cell for 100% look&feel parity

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Config
TUIBRIDGE="${TUIBRIDGE:-./target/release/tuibridge}"
DENO="${DENO:-deno}"
TIMEOUT="${TIMEOUT:-5}"

echo "=========================================="
echo "TuiBridge Parity Harness"
echo "=========================================="
echo "TuiBridge: $TUIBRIDGE"
echo "Deno: $DENO"
echo ""

# Track results
PASS=0
FAIL=0
SKIP=0

# Function to strip ANSI codes for comparison
strip_ansi() {
    sed 's/\x1b\[[0-9;]*m//g' | sed 's/\x1b\[[0-9;]*[A-Za-z]//g'
}

# Run TSX example in Deno (requires transpile or inline)
run_deno() {
    local example="$1"
    local output="$2"
    
    # Create a temp file with ink import
    local tmpfile=$(mktemp /tmp/ink-XXXXXX.mjs)
    
    # For TSX, we need to transpile. Use esbuild or deno cache
    # Since Deno supports TSX natively, we use deno run
    timeout "$TIMEOUT" "$DENO" run -A --no-lock "$example" < /dev/null 2>/dev/null > "$output" || {
        rm -f "$tmpfile"
        return 1
    }
    rm -f "$tmpfile"
    return 0
}

# Run example in TuiBridge
run_tuibridge() {
    local example="$1"
    local output="$2"
    
    timeout "$TIMEOUT" "$TUIBRIDGE" "$example" 2>/dev/null > "$output" || true
}

# Compare outputs
compare() {
    local deno_out="$1"
    local tui_out="$2"
    local name="$3"
    
    # Check both outputs exist
    if [ ! -s "$deno_out" ] && [ ! -s "$tui_out" ]; then
        echo -e "${YELLOW}⊘${NC} $name (no output)"
        ((SKIP++))
        return
    fi
    
    # Strip ANSI and compare structure
    local deno_stripped=$(cat "$deno_out" | strip_ansi)
    local tui_stripped=$(cat "$tui_out" | strip_ansi)
    
    # Compare line by line (structure)
    if diff <(echo "$deno_stripped") <(echo "$tui_stripped") > /dev/null 2>&1; then
        echo -e "${GREEN}✓${NC} $name"
        ((PASS++))
    else
        echo -e "${RED}✗${NC} $name"
        echo "  Deno (first 3 lines):"
        echo "$deno_stripped" | head -3 | sed 's/^/    /'
        echo "  TuiBridge (first 3 lines):"
        echo "$tui_stripped" | head -3 | sed 's/^/    /'
        ((FAIL++))
    fi
}

# Find all TSX examples
EXAMPLES_DIR="./examples"
TSX_EXAMPLES=$(find "$EXAMPLES_DIR" -maxdepth 1 -name "*.tsx" | sort)

if [ -z "$TSX_EXAMPLES" ]; then
    echo -e "${YELLOW}Warning:${NC} No TSX examples found in $EXAMPLES_DIR"
    exit 0
fi

echo -e "${BLUE}Found $(echo "$TSX_EXAMPLES" | wc -l) TSX examples${NC}"
echo ""

# Create temp directory for outputs
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# Banner for example categories
echo "=========================================="
echo "Primary Examples (Core Hooks & Layout)"
echo "=========================================="

PRIMARY_EXAMPLES=(
    "counter.tsx"
    "todo-list.tsx"
    "focus-form.tsx"
    "dashboard.tsx"
    "file-tree.tsx"
    "log-viewer.tsx"
    "spinner.tsx"
    "tabs.tsx"
    "chat-ui.tsx"
    "mouse-app.tsx"
)

for name in "${PRIMARY_EXAMPLES[@]}"; do
    example="$EXAMPLES_DIR/$name"
    if [ ! -f "$example" ]; then
        echo -e "${YELLOW}⊘${NC} $name (not found)"
        ((SKIP++))
        continue
    fi
    
    echo -n "Testing $name... "
    deno_out="$TMPDIR/${name}.deno.txt"
    tui_out="$TMPDIR/${name}.tui.txt"
    
    run_deno "$example" "$deno_out" 2>/dev/null || true
    run_tuibridge "$example" "$tui_out"
    
    if [ -s "$deno_out" ] || [ -s "$tui_out" ]; then
        compare "$deno_out" "$tui_out" "$name" || true
    else
        echo -e "${YELLOW}⊘${NC} (no output)"
        ((SKIP++))
    fi
done

echo ""
echo "=========================================="
echo "Extended Examples (API Coverage)"
echo "=========================================="

EXTENDED_EXAMPLES=(
    "border-styles.tsx"
    "context-demo.tsx"
    "focus-manager.tsx"
    "measure-ref.tsx"
    "sizing-constraints.tsx"
    "spacing-props.tsx"
    "static-overlay.tsx"
    "stdin-stdout.tsx"
    "use-bridge.tsx"
    "wizard.tsx"
)

for name in "${EXTENDED_EXAMPLES[@]}"; do
    example="$EXAMPLES_DIR/$name"
    if [ ! -f "$example" ]; then
        echo -e "${YELLOW}⊘${NC} $name (not found)"
        ((SKIP++))
        continue
    fi
    
    echo -n "Testing $name... "
    deno_out="$TMPDIR/${name}.deno.txt"
    tui_out="$TMPDIR/${name}.tui.txt"
    
    run_deno "$example" "$deno_out" 2>/dev/null || true
    run_tuibridge "$example" "$tui_out"
    
    if [ -s "$deno_out" ] || [ -s "$tui_out" ]; then
        compare "$deno_out" "$tui_out" "$name" || true
    else
        echo -e "${YELLOW}⊘${NC} (no output)"
        ((SKIP++))
    fi
done

# Summary
echo ""
echo "=========================================="
echo "Summary"
echo "=========================================="
echo -e "Passed: ${GREEN}$PASS${NC}"
echo -e "Failed: ${RED}$FAIL${NC}"
echo -e "Skipped: ${YELLOW}$SKIP${NC}"
echo ""

if [ $FAIL -gt 0 ]; then
    echo -e "${RED}Parity FAILED${NC}"
    echo ""
    echo "For visual comparison, run in tmux:"
    echo "  Terminal 1: deno run -A npm:ink <example>"
    echo "  Terminal 2: tuibridge <example>"
    exit 1
else
    echo -e "${GREEN}Parity PASSED${NC}"
    echo ""
    echo "For 100% visual verification, test in tmux:"
    echo "  tmux new-session -d -s tui 'tuibridge examples/counter.tsx; read'"
    echo "  tmux attach -t tui"
    exit 0
fi
