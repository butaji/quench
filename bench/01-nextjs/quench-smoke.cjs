const fs = require('node:fs')
const page = fs.readFileSync('app/page.tsx', 'utf8')
if (!page.includes('To get started, edit')) throw new Error('generated page missing')
console.log('quench smoke: generated Next.js app detected')
