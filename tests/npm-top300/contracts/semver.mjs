import assert from 'node:assert/strict';
import semver from 'semver';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const fixture = name => require(`./upstream-semver/test/fixtures/${name}.js`);
const checks = new Map();
let assertions = 0;
const check = (actual, expected) => { assert.deepEqual(actual, expected); assertions++; };
const call = (name, ...args) => {
  checks.set(name, (checks.get(name) ?? 0) + 1);
  return semver[name](...args);
};
const method = (className, receiver, name, ...args) => {
  const id = `${className}.prototype.${name}`;
  checks.set(id, (checks.get(id) ?? 0) + 1);
  return receiver[name](...args);
};
const construct = (name, ...args) => {
  checks.set(name, (checks.get(name) ?? 0) + 1);
  return new semver[name](...args);
};

for (const [a, b, options] of fixture('comparisons')) {
  for (const [name, positive, negative] of [
    ['compare', 1, -1], ['rcompare', -1, 1], ['gt', true, false],
    ['lt', false, true], ['gte', true, false], ['lte', false, true],
    ['eq', false, false], ['neq', true, true],
  ]) { check(call(name, a, b, options), positive); check(call(name, b, a, options), negative); }
  check(call('cmp', a, '>', b, options), true);
  const av = construct('SemVer', a, options), bv = construct('SemVer', b, options);
  check(method('SemVer', av, 'compare', bv), 1);
  const expectedMain = call('compare', `${av.major}.${av.minor}.${av.patch}`, `${bv.major}.${bv.minor}.${bv.patch}`);
  check(method('SemVer', av, 'compareMain', bv), expectedMain);
  method('SemVer', av, 'comparePre', bv);
  method('SemVer', av, 'compareBuild', bv);
}
for (const [a, b, options] of fixture('equality')) {
  check(call('compare', a, b, options), 0);
  check(call('eq', a, b, options), true);
  check(call('neq', a, b, options), false);
  check(call('cmp', a, '==', b, options), true);
}
for (const [version, major, minor, patch, prerelease, build] of fixture('valid-versions')) {
  const parsed = call('parse', version);
  check([parsed.major, parsed.minor, parsed.patch, parsed.prerelease, parsed.build], [major, minor, patch, prerelease, build]);
  check(call('major', version), major); check(call('minor', version), minor); check(call('patch', version), patch);
  check(call('prerelease', version), prerelease.length ? prerelease : null);
  check(call('valid', version), parsed.version);
  check(method('SemVer', parsed, 'toString'), parsed.version);
  check(method('SemVer', parsed, 'format'), parsed.version);
}
for (const version of fixture('invalid-versions')) {
  check(call('parse', version), null); check(call('valid', version), null);
  assert.throws(() => construct('SemVer', version)); assertions++;
}
for (const [before, release, expected, options, identifier, base] of fixture('increments')) {
  check(call('inc', before, release, options, identifier, base), expected);
  const parsed = call('parse', before, options);
  if (expected) check(method('SemVer', parsed, 'inc', release, identifier, base).version, expected);
  else if (parsed) { assert.throws(() => method('SemVer', parsed, 'inc', release, identifier, base)); assertions++; }
}
for (const [before, release, expected] of fixture('truncations')) check(call('truncate', before, release), expected);
for (const [range, expected, options] of fixture('range-parse')) check(call('validRange', range, options), expected);
for (const [data, expected] of [['range-include', true], ['range-exclude', false]]) {
  for (const [range, version, options] of fixture(data)) {
    check(call('satisfies', version, range, options), expected);
    const r = construct('Range', range, options);
    check(method('Range', r, 'test', version), expected);
    check(method('Range', r, 'toString'), call('validRange', range, options) === '*' ? '' : call('validRange', range, options));
    method('Range', r, 'format');
  }
}
for (const [data, name, expected] of [
  ['version-gt-range', 'gtr', true], ['version-not-gt-range', 'gtr', false],
  ['version-lt-range', 'ltr', true], ['version-not-lt-range', 'ltr', false],
]) for (const [range, version, options] of fixture(data)) check(call(name, version, range, options), expected);
for (const [a, b, expected] of fixture('range-intersection')) {
  check(call('intersects', a, b), expected);
  check(method('Range', construct('Range', a), 'intersects', construct('Range', b)), expected);
}
for (const [a, b, expected, includePrerelease] of fixture('comparator-intersection')) {
  const ac = construct('Comparator', a), bc = construct('Comparator', b);
  check(method('Comparator', ac, 'intersects', bc, { includePrerelease }), expected);
  check(method('Comparator', ac, 'toString'), ac.value);
}
const comparator = construct('Comparator', '>=1.2.3');
check(method('Comparator', comparator, 'test', '1.2.4'), true);
method('Comparator', comparator, 'parse', '<2.0.0');
check(method('Comparator', comparator, 'test', '2.1.0'), false);
check(typeof semver.Comparator.ANY, 'symbol'); checks.set('Comparator.ANY:get', 1);
const range = construct('Range', '^1.2.3');
check(range.range, '>=1.2.3 <2.0.0-0'); checks.set('Range.prototype.range:get', 1);
check(method('Range', range, 'parseRange', '>=1.0.0').map(c => c.value), ['>=1.0.0']);
check(method('SemVer', construct('SemVer', '1.0.0-alpha'), 'comparePre', '1.0.0-beta'), -1);
check(method('SemVer', construct('SemVer', '1.0.0+1'), 'compareBuild', '1.0.0+2'), -1);
check(call('clean', ' =v1.2.3 '), '1.2.3');
check(call('diff', '1.2.3', '2.0.0'), 'major');
check(call('compareLoose', 'v1.2.3', '1.2.3'), 0);
check(call('compareBuild', '1.2.3+1', '1.2.3+2'), -1);
check(call('coerce', 'release 42'), call('parse', '42.0.0'));
check(call('sort', ['2.0.0', '1.0.0']), ['1.0.0', '2.0.0']);
check(call('rsort', ['2.0.0', '1.0.0']), ['2.0.0', '1.0.0']);
check(call('toComparators', '^1.2.3'), [['>=1.2.3', '<2.0.0-0']]);
check(call('maxSatisfying', ['1.0.0', '1.2.3', '2.0.0'], '^1'), '1.2.3');
check(call('minSatisfying', ['1.0.0', '1.2.3', '2.0.0'], '^1'), '1.0.0');
check(call('minVersion', '^1.2.3').version, '1.2.3');
check(call('outside', '2.0.0', '^1', '>'), true);
check(call('simplifyRange', ['1.0.0', '1.1.0', '2.0.0'], '>=1.0.0 <=1.1.0'), '<=1.1.0');
check(call('subset', '^1.2.3', '^1'), true);
check(call('compareIdentifiers', '1', '2'), -1);
check(call('rcompareIdentifiers', '1', '2'), 1);
for (const [name, value] of Object.entries(semver)) {
  if (typeof value === 'function') assert.ok(checks.has(name), `Uncovered semver export: ${name}`);
}
export const coverage = { package: 'semver', assertions, checks: Object.fromEntries([...checks].sort()),
  scope: 'root functions and class methods; upstream data fixtures plus focused contracts; not the entire upstream suite' };
