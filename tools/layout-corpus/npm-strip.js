// Layout training workload (tools/bolt-layout.sh): the npm package under QUENCH_LAYOUT_NODE_MODULES.
const strip = require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/strip-ansi').default || require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/strip-ansi'); const esc = require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/escape-string-regexp').default || require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/escape-string-regexp');
let s = '', n = 0; for (let i = 0; i < 2000; i++) s += '\u001b[' + (30 + i % 8) + 'm' + 'item ' + i + ' (x+y)*[z]? \u001b[0m\n';
const plain = strip(s); for (const w of ['(x+y)', '[z]?', 'item 1', '*', '.']) n += plain.split(new RegExp(esc(w))).length; console.log(plain.length, n);
