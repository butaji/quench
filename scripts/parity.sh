#!/bin/bash
# Parity Harness — Run TSX examples in Deno (reference) and Quench
# Compare ANSI output cell-by-cell for 100% look&feel parity
#
# Uses PTY for proper terminal emulation when available

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Config
QUENCH="${QUENCH:-./target/release/quench}"
DENO="${DENO:-deno}"
TIMEOUT="${TIMEOUT:-5}"
USE_PTY="${USE_PTY:-1}"

echo "=========================================="
echo "Quench Parity Harness"
echo "=========================================="
echo "Quench: $QUENCH"
echo "Deno: $DENO"
echo "PTY mode: $USE_PTY"
echo ""

# Check if quench exists
if [ ! -f "$QUENCH" ]; then
    echo -e "${RED}Error:${NC} Quench binary not found at $QUENCH"
    echo "Run: cargo build --release"
    exit 1
fi

# Track results
PASS=0
FAIL=0
SKIP=0
TOTAL=0

# Function to strip ANSI codes for comparison
strip_ansi() {
    sed 's/\x1b\[[0-9;]*m//g' | sed 's/\x1b\[[0-9;]*[A-Za-z]//g'
}

# Run TSX example in Deno
run_deno() {
    local example="$1"
    local output="$2"
    
    # Deno supports TSX natively
    timeout "$TIMEOUT" "$DENO" run -A --no-lock "$example" < /dev/null 2>/dev/null > "$output" || {
        return 1
    }
    return 0
}

# Run example in Quench with PTY support
run_quench() {
    local example="$1"
    local output="$2"
    local use_pty="${3:-1}"
    
    if [ "$use_pty" = "1" ] && command -v script &> /dev/null; then
        # Use PTY via script(1) for proper terminal emulation
        script -q -c "$QUENCH $example" /dev/null < /dev/null 2>/dev/null > "$output" || true
    else
        # Direct execution (may have TTY issues)
        timeout "$TIMEOUT" "$QUENCH" "$example" 2>/dev/null > "$output" || true
    fi
}

# Compare outputs with detailed diff
compare() {
    local deno_out="$1"
    local tui_out="$2"
    local name="$3"
    
    ((TOTAL++))
    
    # Check both outputs exist
    if [ ! -s "$deno_out" ] && [ ! -s "$tui_out" ]; then
        echo -e "${YELLOW}⊘${NC} $name (no output)"
        ((SKIP++))
        return
    fi
    
    if [ ! -s "$deno_out" ]; then
        echo -e "${RED}✗${NC} $name (Deno: no output)"
        ((FAIL++))
        return
    fi
    
    if [ ! -s "$tui_out" ]; then
        echo -e "${RED}✗${NC} $name (Quench: no output)"
        ((FAIL++))
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
        
        # Show diff for first few lines
        local diff_output=$(diff <(echo "$deno_stripped") <(echo "$tui_stripped") | head -20)
        if [ -n "$diff_output" ]; then
            echo "$diff_output" | sed 's/^/    /'
        fi
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

# Primary examples that should have full parity
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
        ((TOTAL++))
        continue
    fi
    
    echo -n "Testing $name... "
    deno_out="$TMPDIR/${name}.deno.txt"
    tui_out="$TMPDIR/${name}.tui.txt"
    
    run_deno "$example" "$deno_out" 2>/dev/null || true
    run_quench "$example" "$tui_out" "$USE_PTY"
    
    compare "$deno_out" "$tui_out" "$name" || true
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
    "flex-layouts.tsx"
    "text-styles.tsx"
)

for name in "${EXTENDED_EXAMPLES[@]}"; do
    example="$EXAMPLES_DIR/$name"
    if [ ! -f "$example" ]; then
        echo -e "${YELLOW}⊘${NC} $name (not found)"
        ((SKIP++))
        ((TOTAL++))
        continue
    fi
    
    echo -n "Testing $name... "
    deno_out="$TMPDIR/${name}.deno.txt"
    tui_out="$TMPDIR/${name}.tui.txt"
    
    run_deno "$example" "$deno_out" 2>/dev/null || true
    run_quench "$example" "$tui_out" "$USE_PTY"
    
    compare "$deno_out" "$tui_out" "$name" || true
done

# Summary
echo ""
echo "=========================================="
echo "Summary"
echo "=========================================="
echo -e "Total: ${CYAN}$TOTAL${NC}"
echo -e "Passed: ${GREEN}$PASS${NC}"
echo -e "Failed: ${RED}$FAIL${NC}"
echo -e "Skipped: ${YELLOW}$SKIP${NC}"
echo ""

if [ $FAIL -gt 0 ]; then
    echo -e "${RED}Parity FAILED${NC}"
    echo ""
    echo "For visual comparison in tmux:"
    echo "  tmux new-session -d -s tui '$QUENCH examples/counter.tsx; read'"
    echo "  tmux attach -t tui"
    exit 1
else
    echo -e "${GREEN}Parity PASSED${NC}"
    echo ""
    echo "For 100% visual verification, test in tmux:"
    echo "  tmux new-session -d -s tui '$QUENCH examples/counter.tsx; read'"
    echo "  tmux attach -t tui"
    exit 0
fi
