const order = [];

process.nextTick(() => order.push("nextTick"));
Promise.resolve().then(() => order.push("promise"));
setImmediate(() => {
  if (order.join(",") !== "nextTick,promise") {
    throw new Error("nextTick did not run before promise callbacks");
  }
  console.log("next tick order passed");
});
