const observations = [];

function copyPair(input) {
  let first = input;
  let second = first;
  first = { replaced: true };
  return [first.replaced, second === input, first !== second];
}

function assignmentResult(input) {
  let first = 0;
  let second = input;
  return (first = second);
}

function sameSlot(input) {
  let local = input;
  local = local;
  return local;
}

function captured(input) {
  let first = input;
  let second = first;
  return () => second;
}

function loopCopies(limit) {
  let first = 0;
  let second = 0;
  for (let index = 0; index < limit; index++) {
    first = index;
    second = first;
  }
  return [first, second];
}

for (const [name, value] of [
  ["undefined", undefined],
  ["null", null],
  ["number", 42],
  ["string", "copy"],
  ["object", { marker: 17 }],
]) {
  const pair = copyPair(value);
  observations.push([name, pair[0], pair[1], pair[2]]);
}
observations.push(["assignment_result", assignmentResult(23)]);
observations.push(["same_slot", sameSlot("same")]);
observations.push(["captured", captured({ marker: 31 })().marker]);
observations.push(["loop", loopCopies(10)]);

const output = JSON.stringify(observations);
if (typeof console !== "undefined" && console.log) console.log(output);
else print(output);
