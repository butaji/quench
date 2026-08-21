let calls = 0;
const handle = setInterval(() => calls++, 1);
clearInterval(handle);
handle.refresh();

setTimeout(() => {
  clearInterval(handle);
  if (calls !== 0) throw new Error(`interval refresh fired ${calls} times`);
  console.log("interval refresh behavior passed");
}, 5);
