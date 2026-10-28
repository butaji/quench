const observations = [];

function record(name, fn) {
  try {
    observations.push([name, "value", fn()]);
  } catch (error) {
    observations.push([name, "throw", error && error.name]);
  }
}

function ownRead(input) { let local = input; return local.value; }
function inheritedRead(input) { let local = input; return local.inherited; }
function getterRead(input) { let local = input; return local.read; }
function methodRead(input) { let local = input; return local.method(7); }
function proxyRead(input) { let local = input; return local.value; }
function throwingRead(input) {
  let local = input;
  try { return local.read; } catch (error) { return error.message; }
}

const prototype = { inherited: 19 };
const own = Object.create(prototype);
own.value = 23;
own.method = function (value) { return this.value + value; };
record("own", () => ownRead(own));
record("inherited", () => inheritedRead(own));
record("method_receiver", () => methodRead(own));

const getter = { marker: 31 };
Object.defineProperty(getter, "read", {
  get() { return this.marker; },
});
record("getter_receiver", () => getterRead(getter));

let proxyGets = 0;
const proxied = new Proxy({ value: 37 }, {
  get(target, key, receiver) {
    proxyGets++;
    return Reflect.get(target, key, receiver);
  },
});
record("proxy", () => proxyRead(proxied));
observations.push(["proxy_get_count", proxyGets]);

const throwing = {};
Object.defineProperty(throwing, "read", {
  get() { throw new Error("getter sentinel"); },
});
record("caught_getter_throw", () => throwingRead(throwing));
record("null_base", () => ownRead(null));

const output = JSON.stringify(observations);
if (typeof console !== "undefined" && console.log) console.log(output);
else print(output);
