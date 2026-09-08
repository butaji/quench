function divide(left, right) {
  return left / right;
}

function verify(result) {
  if (result !== Infinity) {
    throw new Error("positive infinity division mismatch");
  }
  return result;
}

return { run: divide, arguments: [1, 0], verify: verify };
