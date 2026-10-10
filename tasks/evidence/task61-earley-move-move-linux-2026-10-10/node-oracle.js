const observations = [];

function chain(value, alternate) {
  let first;
  if (alternate) first = value;
  else first = value;
  let second = first;
  let third = second;
  return [first === value, second === value, third === value, first === second];
}

function branches(value) {
  const selected = value ? { tag: "truthy" } : { tag: "falsy" };
  let result = selected;
  const carried = result;
  result = { tag: "replacement" };
  return [carried.tag, result.tag, carried !== result];
}

for (const [name, value] of [
  ["undefined", undefined],
  ["null", null],
  ["false", false],
  ["zero", 0],
  ["number", 17],
  ["string", "move"],
  ["object", { marker: 29 }],
]) {
  observations.push([name, chain(value, true), branches(value)]);
}

const output = JSON.stringify(observations);
if (typeof console !== "undefined" && console.log) console.log(output);
else print(output);
