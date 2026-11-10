'use strict';
// node compare.cjs <c-output> <engine-output> [<engine-output>...]
// Tabulates per-primitive ms against the C baseline, flags checksum mismatches,
// missing primitives, and engine/C ratios above the scoreboard limit.
const fs = require('fs');
const path = require('path');

const RATIO_LIMIT = 5;
const PRIM_LINE = /^PRIM (\S+) (\S+) (\S+)$/;

function parseRun(file) {
  const rows = new Map();
  for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
    const m = PRIM_LINE.exec(line.trim());
    if (m) rows.set(m[1], { ms: Number(m[2]), sum: m[3] });
  }
  return { label: path.basename(file).replace(/\.[^.]*$/, ''), rows };
}

function engineCells(c, row) {
  if (!row) return { cells: ['missing', '-'], ratio: null };
  const ratio = row.ms / c.ms;
  const flags = (row.sum === c.sum ? '' : ' CHECKSUM ' + row.sum) + (ratio > RATIO_LIMIT ? ' >' + RATIO_LIMIT + 'x' : '');
  return { cells: [row.ms.toFixed(1), ratio.toFixed(2) + flags], ratio };
}

function printTable(header, body) {
  const widths = header.map((h, k) => Math.max(h.length, ...body.map((r) => r[k].length)));
  const fmt = (r) => r.map((cell, k) => (k === 0 ? cell.padEnd(widths[k]) : cell.padStart(widths[k]))).join('  ');
  console.log(fmt(header));
  console.log(widths.map((w) => '-'.repeat(w)).join('  '));
  for (const r of body) console.log(fmt(r));
}

function main(argv) {
  if (argv.length < 2) {
    console.error('usage: node compare.cjs <c-output> <engine-output> [<engine-output>...]');
    process.exit(2);
  }
  const [base, ...engines] = argv.map(parseRun);
  const header = ['primitive', 'C ms'];
  for (const e of engines) header.push(e.label + ' ms', e.label + '/C');
  const logs = engines.map(() => ({ sum: 0, count: 0, over: 0, bad: 0 }));
  const body = [];
  for (const [name, c] of base.rows) {
    const row = [name, c.ms.toFixed(1)];
    engines.forEach((e, k) => {
      const { cells, ratio } = engineCells(c, e.rows.get(name));
      row.push(...cells);
      const s = logs[k];
      if (ratio === null || e.rows.get(name).sum !== c.sum) s.bad++;
      if (ratio !== null) { s.sum += Math.log(ratio); s.count++; if (ratio > RATIO_LIMIT) s.over++; }
    });
    body.push(row);
  }
  printTable(header, body);
  console.log('');
  engines.forEach((e, k) => {
    const s = logs[k];
    const geo = s.count ? Math.exp(s.sum / s.count).toFixed(2) : '-';
    console.log(`${e.label}: geomean ${geo}x C, ${s.over}/${base.rows.size} above ${RATIO_LIMIT}x, ${s.bad} missing or mismatched`);
  });
}

main(process.argv.slice(2));
