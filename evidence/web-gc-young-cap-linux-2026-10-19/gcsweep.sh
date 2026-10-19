#!/bin/bash
# gcsweep.sh fw conns secs "ENV=.. ENV=.." ... — peak RSS and req/s per pacing setting (gc-tuning build)
fw=$1; c=$2; d=$3; shift 3
cd /home/user/quench/work/web-upstream
for cfg in "$@"; do
  env $cfg /home/user/quench/target/gct/production/quench-node dist/node/$fw/index.js >/dev/null 2>&1 & p=$!
  for i in $(seq 100); do curl -s --noproxy '*' -o /dev/null http://127.0.0.1:3000/ && break; sleep 0.1; done
  rps=$(/home/user/tools/bombardier -c $c -d ${d}s --fasthttp http://127.0.0.1:3000/ 2>&1 | awk '/Reqs\/sec/{print $2}')
  echo "$fw c=$c [$cfg] rps=$rps peakMB=$(( $(grep VmHWM /proc/$p/status | awk '{print $2}') / 1024 ))"
  kill $p; wait $p 2>/dev/null
done
