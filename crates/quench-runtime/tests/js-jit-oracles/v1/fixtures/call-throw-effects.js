const effects = [];
function invoke(callback) {
  effects.push("before");
  try {
    return callback();
  } finally {
    effects.push("after");
  }
}

function returnsValue() {
  return 19;
}
for (let index = 0; index < 32768; index++) invoke(returnsValue);

const failure = { marker: "call failure" };
function throwsValue() {
  effects.push("callee");
  throw failure;
}
let sameError = false;
try {
  invoke(throwsValue);
} catch (error) {
  sameError = error === failure;
  effects.push("caught");
}
console.log(JSON.stringify({ sameError, effects: effects.slice(-4) }));
