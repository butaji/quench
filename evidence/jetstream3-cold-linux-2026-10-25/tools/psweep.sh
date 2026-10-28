#!/bin/bash
# usage: psweep.sh QBIN OUT ; quench and Bun no-JIT back to back on every test of the default sweep
S=$(dirname $0); Q=$1; OUT=$2; : > $OUT
for t in $(awk '$2=="bun"{print $1}' $S/sweep-default-bun.txt); do
  timeout 3000 python3 $S/rsscmp.py $t $Q /root/.bun/bin/bun >> $OUT 2>&1
done
echo DONE >> $OUT
