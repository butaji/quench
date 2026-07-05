// spec: ECMA-262 sec-object.freeze
// expect: value: true
// tags: objects, freeze

const obj = { a: 1 };
Object.freeze(obj);
try {
  obj.a = 2;
  obj.a === 1;
} catch (e) {
  false;
}
