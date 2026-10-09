let vm;
if (typeof require === "function") {
  try {
    vm = require("node:vm");
  } catch {}
}

const results = [];

function record(name, operation) {
  try {
    results.push({ name, value: operation() });
  } catch (error) {
    results.push({ name, error: error.name, message: error.message });
  }
}

record("fresh-object-and-independent-last-index", () => {
  function make() {
    return /a/g;
  }
  const first = make();
  const second = make();
  const matched = first.exec("a") !== null;
  return {
    distinct: first !== second,
    matched,
    firstLastIndex: first.lastIndex,
    secondLastIndex: second.lastIndex,
  };
});

record("compile-isolation", () => {
  const first = /a/g;
  const second = /a/g;
  first.compile("b", "g");
  return {
    firstMatchesB: first.test("b"),
    secondMatchesA: second.test("a"),
    distinct: first !== second,
  };
});

record("reentrant-exec-during-replace", () => {
  function make() {
    return /a/g;
  }
  const pattern = make();
  let nested = false;
  const value = "a".replace(pattern, () => {
    pattern.lastIndex = 0;
    nested = pattern.exec("a") !== null;
    return "x";
  });
  return { value, nested, lastIndex: pattern.lastIndex };
});

record("cross-object-reentrant-capture-use", () => {
  function make() {
    return /(a)(b)/g;
  }
  const first = make();
  const second = make();
  let nested;
  const value = "ab".replace(first, (whole, firstCapture, secondCapture) => {
    const match = second.exec("ab");
    nested = match && [match[0], match[1], match[2]];
    return `${whole}:${firstCapture}:${secondCapture}`;
  });
  return {
    value,
    distinct: first !== second,
    firstLastIndex: first.lastIndex,
    secondLastIndex: second.lastIndex,
    nested,
  };
});

record("literal-ignores-reassigned-global-regexp", () => {
  const original = globalThis.RegExp;
  try {
    globalThis.RegExp = function () {
      throw new Error("reassigned RegExp was called");
    };
    function make() {
      return /a/;
    }
    return make().test("a");
  } finally {
    globalThis.RegExp = original;
  }
});

if (vm) {
  record("foreign-realm-prototype", () => {
    const context = vm.createContext({});
    const make = vm.runInContext("(function make() { return /a/g; })", context);
    const pattern = make();
    const foreignPrototype = vm.runInContext("RegExp.prototype", context);
    return {
      matches: pattern.test("a"),
      usesForeignPrototype: Object.getPrototypeOf(pattern) === foreignPrototype,
      usesHostPrototype: Object.getPrototypeOf(pattern) === RegExp.prototype,
    };
  });
}

console.log(JSON.stringify(results));
