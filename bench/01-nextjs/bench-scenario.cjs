const fs = require('node:fs')

const scenario = process.argv[2]
const count = 2000
const pagePath = 'app/page.tsx'

function routeLoad() {
  let bytes = 0
  for (let i = 0; i < count; i += 1) bytes += fs.readFileSync(pagePath, 'utf8').length
  return bytes
}

function propsSerialization() {
  let bytes = 0
  for (let i = 0; i < count; i += 1) {
    bytes += JSON.stringify({
      title: 'Next.js example',
      path: '/',
      items: [{ id: i, label: 'Documentation' }, { id: i + 1, label: 'Deploy' }],
    }).length
  }
  return bytes
}

function ssrResponse() {
  let bytes = 0
  for (let i = 0; i < count; i += 1) {
    const props = JSON.stringify({ title: 'Next.js example', request: i })
    const html = `<main><h1>${JSON.parse(props).title}</h1><p>request ${i}</p></main>`
    bytes += html.length
  }
  return bytes
}

const handlers = { 'route-load': routeLoad, 'props-json': propsSerialization, 'ssr-response': ssrResponse }
if (!handlers[scenario]) throw new Error(`unknown scenario: ${scenario}`)
const started = process.hrtime.bigint()
const bytes = handlers[scenario]()
const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6
console.log(JSON.stringify({ scenario, requests: count, elapsedMs, reqPerSec: count / (elapsedMs / 1000), bytes }))
