const event = new CustomEvent("ready", { detail: "done" });
if (event.type !== "ready" || event.detail !== "done") {
  throw new Error("global CustomEvent was not available");
}
