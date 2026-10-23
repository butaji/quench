#!/usr/bin/env node
// Re-adjudicates Quench's Node waivers: each waived pinned Test262 case must
// also fail under the local Node with the pinned harness. With path
// arguments, reports Node's verdict for those cases instead.
//
//   node tools/test262-node-oracle.cjs            # verify the waiver manifest
//   node tools/test262-node-oracle.cjs PATH...    # triage cases (test-relative)
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { REPOSITORY, caseFailures: nodeFailures } = require('./lib/test262-js-host.cjs');

const MANIFEST = path.join(REPOSITORY, 'crates/quench-test262/node-waivers.json');
// Manifest marker for a waived case the oracle cannot run.
const UNVERIFIABLE = 'unverifiable';

async function main() {
  const triage = process.argv.slice(2);
  if (triage.length > 0) {
    for (const relative of triage) {
      const verdict = await nodeFailures(relative);
      const label = verdict === null ? 'node-pass' : verdict.unverifiable ? 'node-unverifiable' : 'node-fail';
      console.log(`${label} ${relative}${verdict ? `  ${JSON.stringify(verdict)}` : ''}`);
    }
    return;
  }
  const manifest = JSON.parse(fs.readFileSync(MANIFEST, 'utf8'));
  if (process.version !== manifest.oracle.version) {
    console.log(`note: manifest adjudicated with ${manifest.oracle.version}; running ${process.version}`);
  }
  let contradicted = 0;
  let byRule = 0;
  let total = 0;
  for (const group of manifest.groups) {
    let groupFailures = 0;
    for (const { path: relative, node } of group.cases) {
      total += 1;
      const verdict = await nodeFailures(relative);
      // A case Node cannot run is waived by its group's Node-arbitrated rule
      // only when the manifest says so; any Node verdict must then be a failure.
      if (node === UNVERIFIABLE && verdict?.unverifiable) {
        byRule += 1;
      } else if (verdict === null || verdict.unverifiable) {
        contradicted += 1;
        console.log(`contradicted ${group.id} ${relative}: ${verdict ? verdict.unverifiable : 'Node passes'}`);
      } else {
        groupFailures += 1;
      }
    }
    console.log(`${group.id}: node fails ${groupFailures}/${group.cases.length}`);
  }
  console.log(`waivers=${total} by-rule=${byRule} contradicted=${contradicted} node=${process.version}`);
  process.exitCode = contradicted === 0 ? 0 : 1;
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 2;
});
