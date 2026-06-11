#!/bin/bash
# Terminal Output Verification
# Opens examples in tmux sessions for visual inspection
# Useful for 100% visual parity verification

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

QUENCH="${QUENCH:-./target/release/quench}"
DENO="${DENO:-deno}"
SESSION_PREFIX="tb_verify"

# Examples to verify
EXAMPLES=(
    "examples/counter.ts"
    "examples/dashboard.ts"
    "examples/spinner.ts"
    "examples/todo-list.ts"
)

echo "=========================================="
echo "Terminal Output Verification"
echo "=========================================="
echo "Quench: $QUENCH"
echo "Deno: $DENO"
echo ""

# Check if tmux is available
if ! command -v tmux &> /dev/null; then
    echo -e "${RED}Error:${NC} tmux is required for visual verification"
    echo "Install: brew install tmux (macOS) or apt install tmux (Linux)"
    exit 1
fi

# Check if quench exists
if [ ! -f "$QUENCH" ]; then
    echo -e "${RED}Error:${NC} Quench binary not found"
    echo "Build: cargo build --release"
    exit 1
fi

# Kill existing verification sessions
echo "Cleaning up existing sessions..."
for ex in "${EXAMPLES[@]}"; do
    name=$(basename "$ex" | sed 's/\..*//')
    session="${SESSION_PREFIX}_${name}"
    tmux kill-session -t "$session" 2>/dev/null || true
done

echo ""
echo "Starting verification sessions..."
echo ""

# Launch Quench examples in separate tmux sessions
for ex in "${EXAMPLES[@]}"; do
    name=$(basename "$ex" | sed 's/\..*//')
    session="${SESSION_PREFIX}_${name}"
    
    echo -e "${CYAN}→${NC} Launching: $name"
    
    # Launch in detached tmux session
    tmux new-session -d -s "$session" "$QUENCH $ex; read -p 'Press Enter to close...'" 2>/dev/null
    
    sleep 0.5  # Stagger launches
done

echo ""
echo "=========================================="
echo "Sessions Launched"
echo "=========================================="
echo ""
echo "Attach to sessions to verify visual output:"
echo ""
for ex in "${EXAMPLES[@]}"; do
    name=$(basename "$ex" | sed 's/\..*//')
    session="${SESSION_PREFIX}_${name}"
    echo -e "  ${GREEN}$name${NC}: tmux attach -t $session"
done
echo ""
echo "To kill all verification sessions:"
echo "  tmux kill-server"
echo ""
echo -e "${BLUE}Tip:${NC} Use Ctrl+B D to detach from a session"
