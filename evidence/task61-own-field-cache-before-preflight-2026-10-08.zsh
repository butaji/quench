#!/bin/zsh
set -euo pipefail
out=target/production/stageb-own-field-write-paired-11-2026-10-08.csv
printf 'operation,build,round,real_seconds,max_rss_bytes,instructions_retired\n' > "$out"
code='function P(){this.x=0;} var o=new P(); function f(){for(var t=0;t<4000;t++) for(var i=0;i<2500;i++) o.x=i;} f(); if(o.x!==2499) throw new Error("write result");'
for round in {1..11}; do
  if (( round % 2 == 1 )); then builds=(baseline candidate); else builds=(candidate baseline); fi
  for build in $builds; do
    if [[ "$build" == baseline ]]; then binary=/tmp/quench-node-array-baseline; else binary=target/production/quench-node; fi
    /usr/bin/time -lp -o /tmp/own-field-pair-time.txt "$binary" -e "$code" >/dev/null 2>/dev/null
    real=$(awk '$1 == "real" { print $2 }' /tmp/own-field-pair-time.txt)
    rss=$(awk '/maximum resident set size/ { print $1 }' /tmp/own-field-pair-time.txt)
    instructions=$(awk '/instructions retired/ { print $1 }' /tmp/own-field-pair-time.txt)
    printf 'own-field-write,%s,%s,%s,%s,%s\n' "$build" "$round" "$real" "$rss" "$instructions" >> "$out"
  done
done
cat "$out"
