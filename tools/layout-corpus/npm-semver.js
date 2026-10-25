// Layout training workload (tools/bolt-layout.sh): the npm package under QUENCH_LAYOUT_NODE_MODULES.
const semver = require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/semver');
const vs = []; for (let a = 0; a < 6; a++) for (let b = 0; b < 12; b++) for (let c = 0; c < 6; c++) vs.push(a + '.' + b + '.' + c + (c % 3 ? '' : '-beta.' + b));
const ranges = ['^1.2.3', '~2.4', '>=3.0.0 <4.0.0-0', '1.x || 3.x', '*', '>1.0.0 <=2.5.0', '4.2 - 5.1.3', '^0.11.0'];
let n = 0; for (const r of ranges) for (const v of vs) if (semver.satisfies(v, r)) n++;
vs.sort(semver.compare); console.log(n, semver.maxSatisfying(vs, '^3.1.0'), vs.length);
