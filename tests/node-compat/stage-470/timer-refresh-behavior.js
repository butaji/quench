let calls = 0;
const handle = setTimeout(() => calls++, 0);
clearTimeout(handle);
if (handle.refresh() !== handle) throw new Error("refresh was not chainable");

setTimeout(() => {
  if (calls !== 0) throw new Error(`refresh fired ${calls} times`);
  console.log("timer refresh behavior passed");
}, 0);
