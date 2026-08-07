const timers = require("timers/promises");

(async () => {
  const controller = new AbortController();
  controller.abort();
  let error;
  try {
    await timers.setTimeout(1, "value", { signal: controller.signal });
  } catch (caught) {
    error = caught;
  }
  if (!error || error.name !== "AbortError" || error.code !== "ABORT_ERR") {
    throw new Error("aborted timers promise had the wrong error");
  }

  console.log("timers promises abort passed");
})();
