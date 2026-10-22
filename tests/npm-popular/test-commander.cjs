'use strict';

const assert = require('node:assert/strict');
const { Command } = require('commander');

const program = new Command();
program
  .exitOverride()
  .option('-n, --name <value>')
  .option('--verbose');
program.parse(['node', 'fixture', '--name', 'quench', '--verbose']);
assert.deepStrictEqual(program.opts(), { name: 'quench', verbose: true });

