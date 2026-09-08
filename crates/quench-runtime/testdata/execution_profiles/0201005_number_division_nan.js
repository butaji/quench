function divide(left, right) {
  return left / right;
}

function verify(result) {
  if (!Number.isNaN(result)) {
    throw new Error("NaN division mismatch");
  }
  return result;
}

return { run: divide, arguments: [0, 0], verify: verify };
