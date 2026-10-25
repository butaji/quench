// Layout training workload (tools/bolt-layout.sh): the npm package under QUENCH_LAYOUT_NODE_MODULES.
const Diff = require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/diff');
const words = 'the quick brown fox jumps over a lazy dog while seven wizards quietly pack boxes of liquor jugs'.split(' ');
const text = (seed) => { let s = '', x = seed; for (let i = 0; i < 400; i++) { x = (x * 1103515245 + 12345) & 0x7fffffff; s += words[x % words.length] + (x % 11 ? ' ' : '.\n'); } return s; };
const a = text(1), b = text(2); console.log(Diff.diffWords(a, b).length, Diff.diffLines(a, b).length, Diff.diffChars(a.slice(0, 800), b.slice(0, 800)).length, Diff.createPatch('f', a, b).length);
