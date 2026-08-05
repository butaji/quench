const url = new URL("http://f:00000000000000/c");
if (url.href !== "http://f:0/c") throw new Error(`Unexpected URL: ${url.href}`);
console.log("URL ports are canonicalized");
