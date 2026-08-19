// Node compat: fs readFileSync + readdirSync.
const fs = require('node:fs');
const path = require('node:path');
const data = fs.readFileSync(path.join('crates', 'quench-node', 'Cargo.toml'));
if (!(data.length > 0)) throw new Error('read-empty');
const entries = fs.readdirSync(path.join('crates', 'quench-node', 'src'));
if (!(entries.length > 0)) throw new Error('readdir-empty');
console.log('fs: ' + data.length + ' ' + entries.length);
