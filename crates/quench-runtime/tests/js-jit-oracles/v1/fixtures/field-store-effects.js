const effects = [];
function updateValue(object, value) {
  object.value = value;
  return object.value;
}

const left = { value: 0 };
const right = { value: 0 };
for (let index = 0; index < 32768; index++) {
  updateValue(index & 1 ? left : right, index);
}

left.value = 101;
right.value = 102;

const setterFailure = { marker: "setter failure" };
const setterObject = Object.defineProperty({}, "value", {
  set(value) {
    effects.push(`setter:${value}`);
    throw setterFailure;
  },
});
let sameError = false;
try {
  updateValue(setterObject, 50001);
} catch (error) {
  sameError = error === setterFailure;
}

const proxy = new Proxy({ value: 0 }, {
  set(target, key, value, receiver) {
    effects.push(`proxy-set:${String(key)}:${value}`);
    return Reflect.set(target, key, value, receiver);
  },
  get(target, key, receiver) {
    effects.push(`proxy-get:${String(key)}`);
    return Reflect.get(target, key, receiver);
  },
});
const proxyValue = updateValue(proxy, 60001);
console.log(JSON.stringify({ values: [left.value, right.value], sameError, proxyValue, effects }));
