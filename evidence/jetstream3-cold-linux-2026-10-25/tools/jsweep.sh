#!/bin/bash
S=$(dirname $0); OUT=$1; : > $OUT
for t in $(awk '$2=="bun"{print $1}' $S/sweep-default-bun.txt); do
  timeout 3000 python3 $S/rsscmp.py $t $S/qjit11 $S/jscjit >> $OUT 2>&1
done
echo DONE >> $OUT
