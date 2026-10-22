#!/bin/zsh
set -euo pipefail

baseline=${1:?usage: paired.zsh BASELINE CANDIDATE [CSV]}
candidate=${2:?usage: paired.zsh BASELINE CANDIDATE [CSV]}
out=${3:-tasks/evidence/task61-mapped-arguments-fast-return-paired-2026-10-08.csv}
time_file="/tmp/task61-mapped-arguments-time-$$.txt"
mkdir -p "${out:h}"
printf 'binary,operation,round,operation_position,pair_order,real_seconds,max_rss_bytes,instructions_retired\n' > "$out"

array_store='var a=[];for(var k=0;k<2500;k++)a[k]=0;function f(a){var x=0;for(var t=0;t<4000;t++)for(var i=0;i<2500;i++){a[i]=i;x+=a[i];}return x;}if(f(a)!==12495000000)throw new Error("array store result");'
local_store='function f(){var x=0;for(var t=0;t<4000;t++)for(var i=0;i<2500;i++)x=i;return x;}if(f()!==2499)throw new Error("local store result");'

measure() {
  local label=$1
  local binary=$2
  local operation=$3
  local source=$4
  local round=$5
  local operation_position=$6
  local pair_order=$7
  /usr/bin/time -lp -o "$time_file" "$binary" -e "$source" >/dev/null 2>/dev/null
  local real=$(awk '$1 == "real" {print $2}' "$time_file")
  local rss=$(awk '/maximum resident set size/ {print $1}' "$time_file")
  local instructions=$(awk '/instructions retired/ {print $1}' "$time_file")
  printf '%s,%s,%s,%s,%s,%s,%s,%s\n' "$label" "$operation" "$round" "$operation_position" "$pair_order" "$real" "$rss" "$instructions" >> "$out"
}

for round in {1..11}; do
  case $(( (round - 1) % 2 )) in
    0) operations=(array_store local_store) ;;
    1) operations=(local_store array_store) ;;
  esac
  position=0
  for operation in $operations; do
    (( position += 1 ))
    case $operation in
      array_store) source=$array_store ;;
      local_store) source=$local_store ;;
    esac
    if (( (round + position) % 2 == 0 )); then
      order=(baseline candidate)
    else
      order=(candidate baseline)
    fi
    for label in $order; do
      case $label in
        baseline) binary=$baseline ;;
        candidate) binary=$candidate ;;
      esac
      measure "$label" "$binary" "$operation" "$source" "$round" "$position" "${order[1]}"
    done
  done
done

cat "$out"
