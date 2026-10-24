const effects = [];
function readValue(object) {
  return object.value;
}

const stable = { value: 7 };
for (let index = 0; index < 32768; index++) readValue(stable);

const getterFailure = { marker: "getter failure" };
const getterObject = Object.defineProperty({}, "value", {
  get() {
    effects.push("getter");
    throw getterFailure;
  },
});
let sameError = false;
try {
  readValue(getterObject);
} catch (error) {
  sameError = error === getterFailure;
}

const proxy = new Proxy({ value: 11 }, {
  get(target, key, receiver) {
    effects.push(`proxy:${String(key)}`);
    return Reflect.get(target, key, receiver);
  },
});
const proxyValue = readValue(proxy);
console.log(JSON.stringify({ sameError, proxyValue, effects }));
