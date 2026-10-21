// Speed and peak RSS of an npm run-npm report (cpu_nanos, wall_nanos, peak_rss_bytes),
// Quench against Node and Bun. Usage: quench-node report.mjs REPORT.json [WORST_COUNT]
import fs from 'node:fs';

const [file, worstArg = '25'] = process.argv.slice(2);
const report = JSON.parse(fs.readFileSync(file, 'utf8'));
const engines = ['quench', 'node', 'bun'];
const MIB = 2 ** 20;
const MS = 1e6;
const verdicts = {};
const rows = [];
for (const record of report.records) {
  verdicts[record.verdict] = (verdicts[record.verdict] ?? 0) + 1;
  if (record.verdict !== 'behavior-pass' && record.verdict !== 'declarations-pass') continue;
  const of = (key, unit) => Object.fromEntries(engines.map(e => [e, record[e][key] / unit]));
  rows.push({ name: record.package.name, wall: of('wall_nanos', MS), cpu: of('cpu_nanos', MS), rss: of('peak_rss_bytes', MIB) });
}
const best = (row, metric) => row[metric].quench < Math.min(row[metric].node, row[metric].bun);
const median = values => {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = sorted.length >> 1;
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
console.log('verdicts', JSON.stringify(verdicts));
console.log(`passing ${rows.length}  fastest wall ${rows.filter(r => best(r, 'wall')).length}  fastest cpu ${rows.filter(r => best(r, 'cpu')).length}  lowest rss ${rows.filter(r => best(r, 'rss')).length}`);
for (const metric of ['wall', 'cpu', 'rss']) {
  const unit = metric === 'rss' ? 'MiB' : 'ms';
  console.log(`${metric} median (${unit}) ` + engines.map(e => `${e} ${median(rows.map(r => r[metric][e])).toFixed(1)}`).join(' '));
}
const ratio = row => row.wall.quench / Math.min(row.wall.node, row.wall.bun);
console.log('slowest relative to the faster of Node and Bun (wall ms):');
for (const row of [...rows].sort((a, b) => ratio(b) - ratio(a)).slice(0, Number(worstArg))) {
  console.log(`  ${row.name.padEnd(40)} q ${row.wall.quench.toFixed(0).padStart(7)} n ${row.wall.node.toFixed(0).padStart(6)} b ${row.wall.bun.toFixed(0).padStart(6)}  x${ratio(row).toFixed(1)}`);
}
const heavier = rows.filter(r => !best(r, 'rss'));
console.log(`not lowest rss: ${heavier.length}`);
for (const row of heavier) {
  console.log(`  ${row.name.padEnd(40)} q ${row.rss.quench.toFixed(1)} n ${row.rss.node.toFixed(1)} b ${row.rss.bun.toFixed(1)}`);
}
