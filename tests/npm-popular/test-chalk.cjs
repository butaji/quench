'use strict';

const assert = require('node:assert/strict');
const chalk = require('chalk');

chalk.level = 1;
assert.strictEqual(chalk.red('failure'), '\u001b[31mfailure\u001b[39m');
assert.strictEqual(chalk.bold.blue('notice'), '\u001b[1m\u001b[34mnotice\u001b[39m\u001b[22m');

