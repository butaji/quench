try {
  new Event();
  throw new Error("Event accepted a missing type");
} catch (error) {
  if (!(error instanceof TypeError)) throw error;
}

const event = new Event("ready");
if (event.type !== "ready") throw new Error("Event type was not preserved");
