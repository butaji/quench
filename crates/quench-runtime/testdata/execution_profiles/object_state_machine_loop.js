function step(state) {
  if (state.kind === 0) {
    state.value = state.value + 3;
    state.kind = 1;
  } else {
    state.value = state.value * 2;
    state.kind = 0;
  }
}

function run(state, count) {
  for (var i = 0; i < count; i++) {
    step(state);
  }
  return state.value;
}

run({ kind: 0, value: 1 }, 2);
var result = run({ kind: 0, value: 3 }, 4);
if (result !== 30) {
  throw new Error("object state-machine mismatch: " + result);
}
return result;
