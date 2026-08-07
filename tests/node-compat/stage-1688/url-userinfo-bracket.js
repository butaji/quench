const url = new URL("http://&a:foo(b]c@d:2/");
if (url.href !== "http://&a:foo(b%5Dc@d:2/") {
  throw new Error(`Unexpected URL: ${url.href}`);
}
console.log("URL userinfo escapes closing brackets");
