#!/bin/bash
# image-vs-c.sh NAME WORKDIR: the C analogue's measurements for the image gate (step `image`),
# one `metric value` line each: bytes, text, rodata, data, bss (size -A), rss_kb (peak resident
# set, GNU time), and ir (callgrind's total, empty only). NAME is a file of
# quench-bench/c-analogues without `.c`, or `empty`: an empty static program, the RSS baseline.
# Built with `gcc -O2 -static`; run with no arguments. Needs GNU time (/usr/bin/time), valgrind.
set -euo pipefail
NAME=$1; W=$(realpath -m "$2"); mkdir -p "$W"
REPO=$(cd "$(dirname "$0")/.." && pwd)
SRC=$REPO/quench-bench/c-analogues/$NAME.c
if [ "$NAME" = empty ]; then SRC=$W/empty.c; echo 'int main(void) { return 0; }' > "$SRC"; fi
BIN=$W/c-$NAME
gcc -O2 -static -o "$BIN" "$SRC"
echo "bytes $(stat -c %s "$BIN")"
size -A "$BIN" | awk '$1==".text"{print "text",$2} $1==".rodata"{print "rodata",$2} $1==".data"{print "data",$2} $1==".bss"{print "bss",$2}'
/usr/bin/time -f %M -o "$W/c-$NAME.rss" "$BIN" > /dev/null 2>&1
echo "rss_kb $(cat "$W/c-$NAME.rss")"
if [ "$NAME" = empty ]; then
  valgrind --tool=callgrind --callgrind-out-file="$W/c-$NAME.cg" "$BIN" > /dev/null 2> "$W/c-$NAME.err"
  echo "ir $(grep -oE 'Collected : [0-9]+' "$W/c-$NAME.err" | awk '{print $3}')"
fi
