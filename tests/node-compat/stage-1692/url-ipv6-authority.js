const url = new URL("http://[2001::1]");
if (url.href !== "http://[2001::1]/") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL IPv6 authorities do not create phantom ports");
