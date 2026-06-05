# Task 005: Run Parity Tests & Fix Issues

## Status: IN PROGRESS

## Running Parity Tests

Use the existing `test_ink_parity_unified.sh` script:

```bash
# List examples
./test_ink_parity_unified.sh --list

# Dry run
./test_ink_parity_unified.sh --dry-run

# Quick test (no compilation)
./test_ink_parity_unified.sh --quick --examples ink-counter ink-todo

# Full test
./test_ink_parity_unified.sh --examples ink-counter ink-todo
```

## Current Test Results

- 88 Ink examples available
- Unit tests passing
- 17 parity harness tests created

## Known Issues

1. New test_ink_parity.sh has execution issues (debugging needed)
2. Some Deno failures expected due to React 19 compatibility

## Next Steps

1. Fix test_ink_parity.sh script
2. Run full parity tests
3. Fix any failing examples
4. Generate coverage report
