#!/usr/bin/env bash
# Enforce the repository Rust function limits that Clippy does not parameterize.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

status=0
while IFS= read -r -d '' file; do
  result="$(awk '
    function count(s,    n) { n=0; while (match(s, /[{}]/)) { n++; s=substr(s,RSTART+1) } return n }
    /^[[:space:]]*(pub([[:space:]]*\([^)]*\))?[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+[A-Za-z_][A-Za-z0-9_]*/ {
      start=NR; brace=0; seen=0; complexity=1
      for (i=1; i<=NF; i++) if ($i ~ /if|match|while|for|&&|\|\|/) complexity++
      brace += count($0); if (brace == 0) seen=1
      next
    }
    start > 0 {
      brace += count($0)
      for (i=1; i<=NF; i++) if ($i ~ /if|match|while|for|&&|\|\|/) complexity++
      if (brace > 0) seen=1
      if (seen && brace == 0) {
        lines=NR-start+1
        if (lines > 40 || complexity > 10) printf "%s:%d: function is %d lines, complexity %d\n", FILENAME,start,lines,complexity
        start=0; brace=0; seen=0
      }
    }
  ' "$file")"
  if [[ -n "$result" ]]; then
    printf '%s\n' "$result" >&2
    status=1
  fi
done < <(find crates -type f -name '*.rs' -print0)

exit "$status"
