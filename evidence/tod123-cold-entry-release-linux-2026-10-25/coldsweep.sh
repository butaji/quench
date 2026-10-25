#!/bin/bash
# coldsweep.sh BIN N "fixtures" "cfg1;cfg2" : median and max RSS of cold runs (fresh text each run), configs interleaved.
b=/tmp/claude-0/$1; n=$2; fx=$3; IFS=';' read -ra cfgs <<< "$4"; R=/home/user/quench/target/release/rss-exec; C=/tmp/claude-0/cold
for f in $fx; do declare -A rs; for c in "${cfgs[@]}"; do rs[$c]=""; done
  for i in $(seq $n); do for c in "${cfgs[@]}"; do
    k=$RANDOM$RANDOM; cp /tmp/claude-0/w/$f.js $C/$f-$k.js; echo "// $k" >> $C/$f-$k.js
    r=$(env QUENCH_MEMORY_POLICY=memory-first $c $R --evict $b -- $b $C/$f-$k.js 2>&1 >/dev/null | tail -1 | sed 's/.*kib=\([0-9]*\).*/\1/'); rs[$c]="${rs[$c]} $r"; rm -f $C/$f-$k.js
  done; done
  for c in "${cfgs[@]}"; do python3 -c "
import statistics as st; r=[int(x) for x in '${rs[$c]}'.split()]; print('$f'.ljust(14), '${c}'.ljust(20), 'cold median', int(st.median(r)), 'max', max(r))"; done; unset rs
done
