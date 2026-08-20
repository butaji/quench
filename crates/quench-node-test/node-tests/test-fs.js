// Node compat: fs module.
const fs = require('node:fs');
const path = require('node:path');
const data = fs.readFileSync('crates/quench-node/Cargo.toml');
if (!(data.length > 0)) throw new Error('read-empty');
const entries = fs.readdirSync('crates/quench-node/src');
if (!(entries.length > 0)) throw new Error('readdir-empty');
console.log('fs: %s %s', data.length, entries.length);
