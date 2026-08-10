const { spawnSync } = require('node:child_process')
const os = require('node:os')

const quench = process.env.QUENCH_BIN || '/Users/admin/Code/GitHub/quench-node/target/debug/quench-node'
const runs = 7
const scenarios = ['route-load', 'props-json', 'ssr-response']

const conditions = {
  platform: `${process.platform}-${process.arch}`,
  cpus: os.cpus().length,
  loadavg: os.loadavg(),
  freeMemoryMiB: os.freemem() / 1024 / 1024,
  totalMemoryMiB: os.totalmem() / 1024 / 1024,
  caffeinate: process.env.BENCH_CAFFEINATE === '1',
}
if (conditions.loadavg[0] > conditions.cpus * 0.8) {
  throw new Error(`system load is too high for a stable run: ${conditions.loadavg[0].toFixed(2)} on ${conditions.cpus} CPUs`)
}
console.log(`conditions\t${JSON.stringify(conditions)}`)

function run(command, args) {
  const timeArgs = process.platform === 'darwin' ? ['-l'] : ['-v']
  const started = process.hrtime.bigint()
  const result = spawnSync('/usr/bin/time', [...timeArgs, command, ...args], { encoding: 'utf8' })
  const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6
  const report = `${result.stderr || ''}\n${result.stdout || ''}`
  const match = report.match(/(\d+)\s+maximum resident set size|Maximum resident set size \(kbytes\):\s*(\d+)/i)
  const rssBytes = match ? Number(match[1] || match[2]) * (match[2] ? 1024 : 1) : 0
  if (result.status !== 0) throw new Error(`${command} failed:\n${report}`)
  return { elapsedMs, rssBytes, result: JSON.parse(result.stdout.trim()) }
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.floor(sorted.length / 2)]
}

const runtimes = [['node', 'node'], ['quench-node', quench]]
console.log('scenario\truntime\tstartup_ms\tpeak_rss_mib\tworkload_ms\treq_sec')
for (const scenario of scenarios) {
  for (const [runtime, executable] of runtimes) {
    run(executable, ['bench-scenario.cjs', scenario])
    const samples = Array.from({ length: runs }, () => run(executable, ['bench-scenario.cjs', scenario]))
    const workloadMs = median(samples.map((sample) => sample.result.elapsedMs))
    console.log([
      scenario,
      runtime,
      median(samples.map((sample) => sample.elapsedMs)).toFixed(1),
      (median(samples.map((sample) => sample.rssBytes)) / 1024 / 1024).toFixed(1),
      workloadMs.toFixed(1),
      (samples[0].result.requests / (workloadMs / 1000)).toFixed(0),
    ].join('\t'))
  }
}
console.log(`\nmedians over ${runs} runs; request/sec is the in-process scenario throughput`)
console.log('real Next.js dev-server startup and HTTP throughput remain Node-only because Next invokes SWC/Turbopack and its Node server runtime')
