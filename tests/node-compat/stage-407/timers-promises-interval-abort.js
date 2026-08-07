const timers = require("timers/promises");

(async () => {
  const controller = new AbortController();
  controller.abort();
  let error;
  try {
    for await (
      const _value of timers.setInterval(1, "value", {
        signal: controller.signal,
      })
    ) {
      throw new Error("aborted interval yielded a value");
    }
  } catch (caught) {
    error = caught;
  }
  if (!error || error.name !== "AbortError" || error.code !== "ABORT_ERR") {
    throw new Error("aborted interval had the wrong error");
  }
  console.log("timers promises interval abort passed");
})();
