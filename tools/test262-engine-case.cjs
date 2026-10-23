#!/usr/bin/env node
// Runs one Test262 case on a Node-compatible engine (Node, Bun) with the pinned
// harness, as one process per case, so a driver can measure each engine's
// process the way it measures Quench's `run-all --case`.
//
//   node tools/test262-engine-case.cjs RELATIVE   # test-relative path
//
// Exit status: 0 pass, 1 fail (the failure on stderr), UNVERIFIABLE_STATUS for a
// case this host cannot run (module cases).
'use strict';

const { caseFailures } = require('./lib/test262-js-host.cjs');

const FAIL_STATUS = 1;
const USAGE_STATUS = 2;
const UNVERIFIABLE_STATUS = 3;

async function main() {
  const relative = process.argv[2];
  if (!relative) {
    console.error('usage: test262-engine-case.cjs RELATIVE');
    return USAGE_STATUS;
  }
  const verdict = await caseFailures(relative);
  if (verdict === null) return 0;
  console.error(JSON.stringify(verdict));
  return verdict.unverifiable ? UNVERIFIABLE_STATUS : FAIL_STATUS;
}

main().then(
  (status) => process.exit(status),
  (error) => {
    console.error(error);
    process.exit(USAGE_STATUS);
  },
);
