const handle = setTimeout(() => {}, 1000);
if (typeof handle.ref !== "function" || typeof handle.unref !== "function")
  throw new Error("timer handle ref methods are missing");
if (!handle.hasRef()) throw new Error("new timer handle should have a ref");
handle.unref();
if (!handle.hasRef()) throw new Error("unref changed scheduling unexpectedly");
clearTimeout(handle);
if (handle.hasRef()) throw new Error("cleared timer should not have a ref");

console.log("timer handle ref passed");
