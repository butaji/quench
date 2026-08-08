const controller = new AbortController();
const request = new Request("http://localhost/", {
  method: "POST",
  body: "hello",
  signal: controller.signal
});

(async () => {
  if (request.signal !== controller.signal) throw new Error("signal missing");
  if (request.bodyUsed) throw new Error("body marked used too early");
  if ((await request.text()) !== "hello")
    throw new Error("request body failed");
  if (!request.bodyUsed) throw new Error("body not marked used");
  if (request.clone) {
    let threw = false;
    try {
      request.clone();
    } catch (_) {
      threw = true;
    }
    if (!threw) throw new Error("consumed body cloned");
  }
  console.log("fetch request body passed");
})();
