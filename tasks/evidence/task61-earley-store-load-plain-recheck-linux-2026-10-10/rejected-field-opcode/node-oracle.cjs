const log = [];
const sentinel = { marker: 41 };
const prototype = {
  get inherited() {
    log.push('inherited');
    return { leaf: 13 };
  },
  get throwing() {
    log.push('throwing');
    throw sentinel;
  },
};
const object = Object.create(prototype);
Object.defineProperty(object, 'own', {
  get() {
    log.push('own');
    return undefined;
  },
});
object.data = 23;

function readInherited(receiver) {
  let value;
  value = receiver.inherited;
  return value.leaf;
}
function readOwn(receiver) {
  let value;
  value = receiver.own;
  return value;
}
function readData(receiver) {
  let value;
  value = receiver.data;
  return value;
}
function readNested(receiver) {
  let value;
  value = receiver.child.leaf;
  return value;
}
function readThrowing(receiver) {
  let value;
  value = receiver.throwing;
  return value;
}
const nested = { child: { leaf: 29 } };
let caughtIdentity = false;
try {
  readThrowing(object);
} catch (error) {
  caughtIdentity = error === sentinel;
}
console.log(JSON.stringify({
  inherited: readInherited(object),
  ownIsUndefined: readOwn(object) === undefined,
  data: readData(object),
  nested: readNested(nested),
  caughtIdentity,
  log,
}));
