import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

export function exercise(requireFromString) {
  let assertions = 0;
  const equal = (actual, expected) => { assert.deepEqual(actual, expected); assertions++; };
  const throws = (action, error) => { assert.throws(action, error); assertions++; };
  const filename = path.resolve('compiled.cjs');
  equal(requireFromString('module.exports = 42'), 42);
  equal(requireFromString('exports.answer = 42'), { answer: 42 });
  equal(requireFromString('module.exports = [__filename, __dirname, arguments.length, this === exports]', filename),
    [filename, path.dirname(filename), 5, true]);
  equal(requireFromString('module.exports = require("node:path").basename(__filename)', filename), 'compiled.cjs');
  fs.writeFileSync('compiled-dependency.cjs', 'module.exports = { answer: 42 };');
  equal(requireFromString('module.exports = require("./compiled-dependency.cjs")', filename), { answer: 42 });
  equal(requireFromString('#!/usr/bin/env node\nmodule.exports = 7', filename), 7);
  equal(requireFromString('\uFEFFmodule.exports = 8', filename), 8);
  equal(requireFromString('module.exports = null', {}), null);
  equal(requireFromString(''), {});
  throws(() => requireFromString(1), /code must be a string, not number/);
  throws(() => requireFromString('const ='), SyntaxError);
  throws(() => requireFromString('throw new RangeError("compile failure")'), RangeError);
  return { package: 'require-from-string', assertions, checks: { default: assertions },
    scope: 'Default function: compilation, exports, filename, builtin/relative require, shebang/BOM and errors; custom lookup paths remain unqualified' };
}
