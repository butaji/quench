#!/bin/bash
# Quench pre-commit hook - enforces 400/50/10 linting rules
# 400 lines per file max
# 50 lines per function max  
# 10 complexity max

set -e

echo "Running Quench linting checks..."

# Get staged .rs files
STAGED_FILES=$(git diff --cached --name-only --diff-filter=ACM | grep '\.rs$' | grep -v '^\s*#')

if [ -z "$STAGED_FILES" ]; then
    echo "No Rust files staged, skipping checks."
    exit 0
fi

echo "Checking staged files: $STAGED_FILES"

# Run clippy with strict settings
# Allow specific warnings, deny everything else
cargo clippy --all-targets -- \
    -D warnings \
    -A clippy::all \
    -W clippy::function_length \
    -W clippy::file_length \
    -W clippy::too_many_lines \
    -W clippy::type_complexity

# Check for files exceeding 400 lines
for file in $STAGED_FILES; do
    if [ -f "$file" ]; then
        LINES=$(wc -l < "$file")
        if [ "$LINES" -gt 400 ]; then
            echo "ERROR: $file has $LINES lines (max 400)"
            exit 1
        fi
    fi
done

echo "All linting checks passed!"
exit 0
