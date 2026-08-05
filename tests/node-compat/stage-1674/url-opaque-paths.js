const url = new URL("a:\t foo.com", "http://example.org/foo/bar");
if (url.href !== "a: foo.com") throw new Error(`Unexpected URL: ${url.href}`);
console.log("URL opaque paths preserve spaces");
