import assert from 'node:assert/strict';
import fs from 'node:fs';

export async function exercise(glob) {
  const checks = {};
  let assertions = 0;
  const equal = (id, actual, expected) => {
    assert.deepEqual(actual, expected); assertions++;
    checks[id] = (checks[id] ?? 0) + 1;
  };
  const throws = (id, action) => {
    assert.throws(action, TypeError); assertions++;
    checks[id] = (checks[id] ?? 0) + 1;
  };
  fs.mkdirSync('glob-contract', { recursive: true });
  fs.writeFileSync('glob-contract/a.js', 'a');
  fs.writeFileSync('glob-contract/b.js', 'b');
  fs.writeFileSync('glob-contract/skip.js', 'skip');
  const patterns = ['glob-contract/*.js', '!glob-contract/skip.js'];
  const expected = ['glob-contract/a.js', 'glob-contract/b.js'];
  equal('', (await glob(patterns)).sort(), expected);
  equal('sync', glob.sync(patterns).sort(), expected);
  equal('sync', glob.sync('glob-contract/missing.js'), []);
  const streamed = [];
  for await (const entry of glob.stream(patterns)) streamed.push(entry);
  equal('stream', streamed.sort(), expected);
  equal('generateTasks', glob.generateTasks(patterns), [{ dynamic: true,
    positive: ['glob-contract/*.js'], negative: ['glob-contract/skip.js'],
    base: 'glob-contract', patterns }]);
  equal('isDynamicPattern', glob.isDynamicPattern('glob-contract/a.js'), false);
  equal('isDynamicPattern', glob.isDynamicPattern('glob-contract/*.js'), true);
  equal('glob', glob.glob, glob);
  equal('async', glob.async, glob);
  equal('globSync', glob.globSync, glob.sync);
  equal('globStream', glob.globStream, glob.stream);
  for (const [prefix, api, escaped] of [
    ['', glob, 'a/\\[b\\]\\(c\\)\\*\\?.txt'],
    ['posix.', glob.posix, 'a/\\[b\\]\\(c\\)\\*\\?.txt'],
    ['win32.', glob.win32, 'a/\\[b\\]\\(c\\)*?.txt'],
  ]) {
    for (const name of ['escapePath', 'convertPathToPattern']) {
      equal(prefix + name, api[name]('a/[b](c)*?.txt'), escaped);
      throws(prefix + name, () => api[name](''));
    }
  }
  throws('sync', () => glob.sync(null));
  throws('stream', () => glob.stream(1));
  throws('generateTasks', () => glob.generateTasks(['']));
  throws('isDynamicPattern', () => glob.isDynamicPattern(''));
  let rejection;
  try { await glob(''); } catch (error) { rejection = error; }
  equal('', rejection instanceof TypeError, true);
  return { package: 'fast-glob', assertions, checks,
    scope: 'Pinned public callable and attached-function corpus, with filesystem, stream and error checks; returned stream APIs and other inputs remain separate obligations' };
}
