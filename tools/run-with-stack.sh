#!/bin/bash
# Increase stack limit before running the test binary.
# The tree-walking interpreter needs more than the default 8MB macOS stack.
ulimit -s unlimited 2>/dev/null
exec "$@"
