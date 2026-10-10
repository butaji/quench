const events = [];
const sentinel = { marker: 'unary-throw' };

const coercible = {
  [Symbol.toPrimitive](hint) {
    events.push(`coerce:${hint}`);
    return 2;
  },
};

const throwing = {
  [Symbol.toPrimitive](hint) {
    events.push(`throw:${hint}`);
    throw sentinel;
  },
};

function plusBranch(value) {
  if (+value) return 'taken';
  return 'skipped';
}

function minusBranch(value) {
  if (-value) return 'taken';
  return 'skipped';
}

function notBranch(value) {
  if (!value) return 'taken';
  return 'skipped';
}

function bitNotBranch(value) {
  if (~value) return 'taken';
  return 'skipped';
}

function typeofBranch(value) {
  if (typeof value) return 'taken';
  return 'skipped';
}

function voidBranch(value) {
  if (void value) return 'taken';
  return 'skipped';
}

function deleteBranch(value) {
  if (delete value.x) return 'taken';
  return 'skipped';
}

function capture(label, callback) {
  try {
    return [label, callback()];
  } catch (error) {
    return [label, error === sentinel ? 'sentinel' : `${error.name}:${error.message}`];
  }
}

const deletable = { x: 1 };
const results = [
  capture('plus', () => plusBranch(coercible)),
  capture('minus', () => minusBranch(coercible)),
  capture('not', () => notBranch(coercible)),
  capture('bit-not', () => bitNotBranch(coercible)),
  capture('typeof', () => typeofBranch(coercible)),
  capture('void', () => voidBranch(coercible)),
  capture('delete', () => deleteBranch(deletable)),
  capture('plus-throw', () => plusBranch(throwing)),
];

console.log(JSON.stringify({ results, events, deleted: !('x' in deletable) }));
