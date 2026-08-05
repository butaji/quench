const url = new URL("http://f:00000000000000000000080/c");
if (url.href !== "http://f/c") throw new Error(`Unexpected URL: ${url.href}`);
console.log("URL default ports are omitted");
