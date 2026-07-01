# Makefile for Quench - ensures timeout-protected test runs
#
# Use these targets instead of 'cargo test' directly to ensure timeout protection.

.PHONY: test test-runtime test-quick test-conformance check build clean help

# Default test target - uses xtask with 5min timeout (300s)
test:
	@./scripts/run_tests.sh

# Run runtime tests only (3min timeout)
test-runtime:
	@./scripts/run_tests.sh -p quench-runtime --test runtime_tests

# Run quick tests with shorter timeout (1min)
test-quick:
	@./scripts/run_tests.sh -p quench-runtime --test runtime_tests -- test_runtime_loads

# Run all conformance tests (10min timeout)
test-conformance:
	@./scripts/run_tests.sh -p quench-runtime --test conformance -- --ignored

# Type check
check:
	@./scripts/run_tests.sh -- --check 2>/dev/null || cargo check --all-targets

# Build
build:
	@cargo build

# Clean
clean:
	@cargo clean

# Run specific test by name
rt %:
	@./scripts/run_tests.sh -- "$@"

# Help
help:
	@echo "Quench Makefile - timeout-protected test targets"
	@echo ""
	@echo "  make test              Run all tests (5min timeout)"
	@echo "  make test-runtime      Run runtime tests (3min timeout)"
	@echo "  make test-quick        Run quick tests (1min timeout)"
	@echo "  make test-conformance  Run conformance suite (10min timeout)"
	@echo "  make check             Type check"
	@echo "  make build             Build"
	@echo "  make clean             Clean build"
	@echo "  make rt <name>         Run specific test"
