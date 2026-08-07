for (const schedule of [setTimeout, setImmediate, setInterval]) {
  let error;
  try {
    schedule(null, 0);
  } catch (caught) {
    error = caught;
  }
  if (!error || error.name !== "TypeError") {
    throw new Error("timer callback validation was not synchronous");
  }
}

console.log("timer callback validation passed");
