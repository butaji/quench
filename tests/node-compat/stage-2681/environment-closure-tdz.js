"use strict";

function makeReader() {
  const read = () => initialized;
  const initialized = "ready";
  return read;
}

if (makeReader()() !== "ready") {
  throw new Error("captured binding was not initialized");
}
