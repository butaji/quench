#!/bin/bash
# Run TypeScript's own test suite
# Usage: ./scripts/run_typescript_runner.sh [test-type] [category]
#
# Examples:
#   ./scripts/run_typescript_runner.sh                  # Run conformance tests
#   ./scripts/run_typescript_runner.sh conformance      # Run conformance
#   ./scripts/run_typescript_runner.sh compiler        # Run compiler tests
#   ./scripts/run_typescript_runner.sh project         # Run project tests
#
# Options:
#   --dirty    Update baseline files
#   --help     Show this help

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TS_DIR="$SCRIPT_DIR/../tests/typescript"
TEST_TYPE="${1:-conformance}"
CATEGORY="${2:-expressions}"
DIRTY=""
HELP=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --dirty)
            DIRTY="--dirty"
            shift
            ;;
        --help|-h)
            HELP="yes"
            shift
            ;;
        *)
            TEST_TYPE="$1"
            if [[ $# -gt 1 ]]; then
                CATEGORY="$2"
                shift
            fi
            shift
            ;;
    esac
done

if [[ "$HELP" == "yes" ]]; then
    head -20 "$0" | tail -15
    exit 0
fi

# Check that the TypeScript directory exists
if [[ ! -d "$TS_DIR" ]]; then
    echo "Error: TypeScript directory not found at $TS_DIR"
    echo "Have you initialized the submodule?"
    echo "  git submodule update --init tests/typescript"
    exit 1
fi

cd "$TS_DIR"

# Install dependencies if needed
if [[ ! -d "node_modules" ]]; then
    echo "Installing TypeScript dependencies..."
    npm ci
fi

# Build the test runner
echo "Building TypeScript test runner..."
npx hereby build:tests

# Run the tests
echo "Running TypeScript tests..."
echo "Test type: $TEST_TYPE"
echo "Category: $CATEGORY"
echo ""

case "$TEST_TYPE" in
    conformance)
        npx hereby runtests --tests=conformance --test="$CATEGORY" $DIRTY
        ;;
    compiler)
        npx hereby runtests --tests=compiler $DIRTY
        ;;
    project)
        npx hereby runtests --tests=project $DIRTY
        ;;
    all)
        npx hereby runtests $DIRTY
        ;;
    *)
        echo "Unknown test type: $TEST_TYPE"
        echo "Valid types: conformance, compiler, project, all"
        exit 1
        ;;
esac

echo ""
echo "Done!"
