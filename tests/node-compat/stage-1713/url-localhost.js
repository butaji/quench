const url = new URL("file://localhost");
if (url.href !== "file:///") throw new Error(`Unexpected URL: ${url.href}`);
console.log("URL file localhost authorities are normalized");
