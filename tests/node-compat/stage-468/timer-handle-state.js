const handle = setTimeout(() => {}, 1000);
if (!handle.hasRef()) throw new Error("timer should start referenced");
if (handle.unref() !== handle || handle.hasRef()) {
  throw new Error("unref state was incorrect");
}
if (handle.ref() !== handle || !handle.hasRef()) {
  throw new Error("ref state was incorrect");
}
clearTimeout(handle);
if (handle.hasRef()) throw new Error("cleared timer remained referenced");

console.log("timer handle state passed");
