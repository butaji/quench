function choose(value, fallback) {
  return value ?? (fallback ? 41 : 7);
}

choose(1, false);
function verify(result) {
  if (choose(0, true) !== 0) throw new Error("non-nullish zero mismatch");
  if (choose(null, true) !== 41) throw new Error("truthy fallback mismatch");
  if (choose(null, false) !== 7) throw new Error("falsy fallback mismatch");
  if (result !== 7) throw new Error("measured falsy fallback mismatch");
  return result;
}
return { run: choose, verify: verify, arguments: [null, false] };
