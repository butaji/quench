const live = [];
function enter(callback, depth) {
  const retained = [depth, { depth }];
  live.push(retained);
  if (depth === 0) return retained[1].depth;
  return callback(depth - 1) + retained[1].depth;
}

function callback(depth) {
  return enter(callback, depth);
}

for (let index = 0; index < 32768; index++) enter(callback, 1);
const before = live.length;
const result = enter(callback, 3);
console.log(JSON.stringify({ result, allocationsRetained: live.length - before }));
