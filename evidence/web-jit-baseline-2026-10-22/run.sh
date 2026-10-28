#!/bin/bash
# jit.sh runtime fw path secs : req/s with the runtime's default JIT
rt=$1; fw=$2; path=$3; secs=${4:-6}
cd /home/user/quench/work/web-upstream
case $rt in
  node) cmd="/home/user/tools/node-v26.12.0-linux-x64/bin/node dist/node/$fw/index.js";;
  quench) cmd="/home/user/quench/target/production/quench-node --jit dist/node/$fw/index.js";;
  quench-nojit) cmd="/home/user/quench/target/production/quench-node dist/node/$fw/index.js";;
  bun) if [ $fw = hono ]; then cmd="bun src/bun/hono.ts"; else cmd="bun dist/node/$fw/index.js"; fi;;
esac
$cmd >/dev/null 2>&1 &
pid=$!
for i in $(seq 300); do curl -s --noproxy '*' -o /dev/null http://127.0.0.1:3000/ && break; sleep 0.1; done
extra=""; [ "$path" = /json ] && extra="-m POST -H Content-Type:application/json -f scripts/body.json"
/home/user/tools/bombardier -c 500 -d 2s --fasthttp $extra http://127.0.0.1:3000$path >/dev/null 2>&1
r=$(/home/user/tools/bombardier -c 500 -d ${secs}s --fasthttp $extra http://127.0.0.1:3000$path 2>&1 | grep -E "Reqs/sec" | awk '{print $2}')
echo "$rt $fw $path $r"
kill $pid; wait $pid 2>/dev/null
