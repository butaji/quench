#!/bin/bash
# usage: runut.sh BIN OUT
cd /tmp/claude-0/-home-user-quench/60adca64-dc01-5dd8-b1ca-f2f60af794ba/scratchpad/ut
for f in test/parallel/test-buffer*.js; do flags=$(grep -o "^// Flags: .*" $f | sed 's#// Flags: ##'); timeout 60 $1 $flags $f >/dev/null 2>&1; echo "$? $f"; done > $2
