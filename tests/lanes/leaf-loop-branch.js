function selectInto(values, output) {
  for (let i = 0; i < values.length; i++) {
    const value = values[i];
    if (value.keep) output.push(value);
  }
}

for (let round = 0; round < 10000; round++) {
  const output = [];
  selectInto([{ keep: false }, { keep: true }], output);
  if (output.length !== 1 || output[0].keep !== true) {
    throw new Error("leaf loop truncated conditional work");
  }
}
