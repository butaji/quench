const url = new URL("http://[0:0:0:0:0:0:13.1.68.3]");
if (url.href !== "http://[::d01:4403]/") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL IPv6 zero runs are compressed");
