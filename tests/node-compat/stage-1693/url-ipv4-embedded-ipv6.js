const url = new URL("http://[::127.0.0.1]");
if (url.href !== "http://[::7f00:1]/") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL IPv4-embedded IPv6 hosts are canonicalized");
