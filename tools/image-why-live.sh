#!/usr/bin/env bash
# image-why-live.sh ENTRY [BINARY] [WORKDIR]: the retained code of an AOT executable by mechanism
# (the compile report's `retained_mechanisms`), then for each nonzero mechanism the linker's
# --why-live chain to that mechanism's largest symbol (QUENCH_EXTRA_LINK_ARGS relinks the image
# with the flag; the chains are the linker's stderr, kept in WORKDIR/why-MECHANISM.log).
# The chains name the next retention root to cut.
#
# usage: tools/image-why-live.sh ENTRY [BINARY] [WORKDIR]   (default BINARY: target/experiment/quench)
set -euo pipefail
ENTRY=$(realpath "$1"); BIN=$(realpath "${2:-target/experiment/quench}")
W=$(realpath -m "${3:-$(mktemp -d)}"); mkdir -p "$W"

"$BIN" compile "$ENTRY" -o "$W/image" --report "$W/report.json" > /dev/null
python3 - "$W/report.json" > "$W/mechanisms.tsv" <<'PY'
import json, sys
report = json.load(open(sys.argv[1]))
for name, m in sorted(report["retained_mechanisms"].items(), key=lambda kv: -kv[1]["bytes"]):
    print(f'{name}\t{m["bytes"]}\t{m["linker_symbol"]}\t{m["representative"]}')
PY
echo "== retained mechanisms (bytes, largest symbol)"
while IFS=$'\t' read -r name bytes _ rep; do
  printf '%-18s %10s  %s\n' "$name" "$bytes" "$rep"
done < "$W/mechanisms.tsv"

while IFS=$'\t' read -r name bytes sym rep; do
  [ "$bytes" -gt 0 ] || continue
  echo "== $name: why-live $rep"
  if [ -z "$sym" ]; then echo "   no linker name at the representative's address"; continue; fi
  QUENCH_EXTRA_LINK_ARGS="-Wl,--why-live=$sym" "$BIN" compile "$ENTRY" -o "$W/why-$name" \
    --report "$W/why-$name.json" > "$W/why-$name.log" 2>&1 || echo "   relink with --why-live failed; see $W/why-$name.log"
  grep -i "why" "$W/why-$name.log" || true
done < "$W/mechanisms.tsv"
echo "files in $W"
