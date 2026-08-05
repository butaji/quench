const url = new URL("foo://", "http://example.org/foo/bar");
if (url.href !== "foo://") throw new Error(`Unexpected URL: ${url.href}`);
console.log("Unknown URL schemes preserve empty authorities");
