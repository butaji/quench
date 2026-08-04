#!/usr/bin/env sh
set -eu

git config core.hooksPath .githooks
echo "Installed local hooks: core.hooksPath=.githooks"
