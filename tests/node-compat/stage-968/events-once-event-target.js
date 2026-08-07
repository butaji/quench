const { once } = require("events");

async function eventTargetDelivery() {
  const target = new EventTarget();
  const expected = new Event("message");
  queueMicrotask(() => target.dispatchEvent(expected));
  const [actual] = await once(target, "message");
  if (actual !== expected) {
    throw new Error("EventTarget payload was not preserved");
  }
}

async function invalidOptions() {
  const invalid = [1, "options", null, false, () => {}, Symbol("options")];
  for (const options of invalid) {
    try {
      await once(new EventTarget(), "message", options);
      throw new Error("invalid options were accepted");
    } catch (error) {
      if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
    }
  }
}

(async () => {
  await eventTargetDelivery();
  await invalidOptions();
})();
