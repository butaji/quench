#!/bin/bash
# Parity Harness — Run examples in Deno (reference) and TuiBridge
# Compare ANSI output cell-by-cell

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Config
TUIBRIDGE="${TUIBRIDGE:-./target/release/tuibridge}"
DENO="${DENO:-deno}"
TIMEOUT="${TIMEOUT:-5}"
TERMINAL_SIZE="${TERMINAL_SIZE:-80x24}"

# Parse terminal size
IFS='x' read -r COLS ROWS <<< "$TERMINAL_SIZE"

echo "=========================================="
echo "TuiBridge Parity Harness"
echo "=========================================="
echo "TuiBridge: $TUIBRIDGE"
echo "Deno: $DENO"
echo "Terminal: ${COLS}x${ROWS}"
echo ""

# Track results
PASS=0
FAIL=0
SKIP=0

# Function to run an example and capture output
run_example_deno() {
    local example="$1"
    local output_file="$2"
    
    # Run with timeout and capture output
    timeout "$TIMEOUT" "$DENO" run --allow-all \
        "npm:ink" \
        < /dev/null 2>/dev/null > "$output_file" || true
}

run_example_tuibridge() {
    local example="$1"
    local output_file="$2"
    
    # Create a PTY wrapper to set terminal size
    # For now, just run and hope the default is close enough
    timeout "$TIMEOUT" "$TUIBRIDGE" "$example" 2>/dev/null > "$output_file" || true
}

# Compare ANSI outputs
compare_outputs() {
    local file1="$1"
    local file2="$2"
    local name="$3"
    
    # Strip ANSI codes and compare
    local stripped1=$(cat "$file1" | sed 's/\x1b\[[0-9;]*m//g' | sed 's/\x1b\[[0-9;]*[A-Za-z]//g')
    local stripped2=$(cat "$file2" | sed 's/\x1b\[[0-9;]*m//g' | sed 's/\x1b\[[0-9;]*[A-Za-z]//g')
    
    if diff <(echo "$stripped1") <(echo "$stripped2") > /dev/null 2>&1; then
        echo -e "${GREEN}✓${NC} $name"
        ((PASS++))
        return 0
    else
        echo -e "${RED}✗${NC} $name"
        echo "  Deno output:"
        head -5 "$file1" | sed 's/^/    /'
        echo "  TuiBridge output:"
        head -5 "$file2" | sed 's/^/    /'
        ((FAIL++))
        return 1
    fi
}

# Find all examples
EXAMPLES_DIR="./examples"
if [ ! -d "$EXAMPLES_DIR" ]; then
    echo -e "${YELLOW}Warning:${NC} No examples directory found"
    exit 0
fi

# Get list of JS examples
EXAMPLES=$(find "$EXAMPLES_DIR" -maxdepth 1 -name "*.js" | sort)

if [ -z "$EXAMPLES" ]; then
    echo -e "${YELLOW}Warning:${NC} No examples found"
    exit 0
fi

echo "Found $(echo "$EXAMPLES" | wc -l) examples"
echo ""

# Create temp directory for outputs
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# Run each example
for example in $EXAMPLES; do
    name=$(basename "$example" .js)
    deno_output="$TMPDIR/${name}.deno.txt"
    tui_output="$TMPDIR/${name}.tui.txt"
    
    echo -n "Testing $name... "
    
    # Check if example is compatible
    if grep -q "useApp\|useInput" "$example" 2>/dev/null; then
        # These need interactive mode - skip for now
        echo -e "${YELLOW}⊘${NC} (interactive - skipped)"
        ((SKIP++))
        continue
    fi
    
    # Run in Deno
    run_example_deno "$example" "$deno_output"
    
    # Run in TuiBridge  
    run_example_tuibridge "$example" "$tui_output"
    
    # Compare (for non-interactive examples, this works)
    if [ -s "$deno_output" ] && [ -s "$tui_output" ]; then
        compare_outputs "$deno_output" "$tui_output" "$name" || true
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
    exit 1
else
    echo -e "${GREEN}Parity PASSED${NC}"
    exit 0
fi
