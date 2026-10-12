#!/usr/bin/env sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tests_dir=$(dirname -- "$script_dir")
destination="$tests_dir/node_modules"

if [ -e "$destination" ]; then
  printf '%s already exists; refusing to replace local test dependencies\n' "$destination" >&2
  exit 1
fi

npm ci --prefix "$script_dir" --ignore-scripts
mv "$script_dir/node_modules" "$destination"
