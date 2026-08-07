const url = new URL("http://::@c@d:2");
if (url.href !== "http://:%3A%40c@d:2/") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL userinfo delimiters are parsed correctly");
