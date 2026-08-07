const url = new URL("/\\server/file", "file:///tmp/mock/path");
if (url.href !== "file://server/file") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL mixed UNC prefixes resolve as network hosts");
