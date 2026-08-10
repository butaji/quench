const fs = require('node:fs')

const started = process.hrtime.bigint()
let checksum = 0
for (let iteration = 0; iteration < 1000; iteration += 1) {
  const page = fs.readFileSync('app/page.tsx', 'utf8')
  const props = JSON.stringify({ title: 'Next.js example', iteration })
  checksum += page.length + props.length
  if (!page.includes('To get started, edit')) throw new Error('route check failed')
}
const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6
console.log(JSON.stringify({ elapsedMs, operations: 1000, checksum }))
