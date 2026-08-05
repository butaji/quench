const url = new URL("//", "file:///tmp/mock/path");
if (url.href !== "file:///") throw new Error(`Unexpected URL: ${url.href}`);
console.log("URL empty file authorities receive a root path");
