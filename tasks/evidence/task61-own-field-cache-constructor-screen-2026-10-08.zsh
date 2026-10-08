#!/bin/zsh
set -euo pipefail
out=target/production/stageb-constructor-candidate-screen-2026-10-08.csv
printf 'operation,build,round,real_seconds,max_rss_bytes,instructions_retired\n' > "$out"
code='function P(){this.x=1;this.y=2;} var last; for(var i=0;i<1000000;i++) last=new P(); if(last.x!==1||last.y!==2) throw new Error("constructor result");'
for round in {1..3}; do
  if (( round % 2 == 1 )); then builds=(baseline candidate); else builds=(candidate baseline); fi
  for build in $builds; do
    if [[ "$build" == baseline ]]; then binary=/tmp/quench-node-array-baseline; else binary=target/production/quench-node; fi
    /usr/bin/time -lp -o /tmp/construct-candidate-screen-time.txt "$binary" -e "$code" >/dev/null 2>/dev/null
    real=$(awk '$1 == "real" { print $2 }' /tmp/construct-candidate-screen-time.txt)
    rss=$(awk '/maximum resident set size/ { print $1 }' /tmp/construct-candidate-screen-time.txt)
    instructions=$(awk '/instructions retired/ { print $1 }' /tmp/construct-candidate-screen-time.txt)
    printf 'two-field-constructor,%s,%s,%s,%s,%s\n' "$build" "$round" "$real" "$rss" "$instructions" >> "$out"
  done
done
cat "$out"
