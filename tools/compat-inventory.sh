#!/bin/sh
set -eu
exec node "$(dirname "$0")/compat-inventory.cjs" "${1:-target/compat/inventory.json}"
