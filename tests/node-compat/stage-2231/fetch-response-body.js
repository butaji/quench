const response = new Response('{"ok":true}', {
  status: 201,
  headers: { "content-type": "application/json" }
});

(async () => {
  if (!(await response.json()).ok) throw new Error("json failed");
  const clone = response.clone();
  if ((await clone.text()) !== '{"ok":true}') {
    throw new Error("clone failed");
  }
  const bytes = await new Response("abc").bytes();
  if (bytes.length !== 3 || new TextDecoder().decode(bytes) !== "abc") {
    throw new Error("bytes failed");
  }
  const blob = await new Response("abc").blob();
  if ((await blob.text()) !== "abc") throw new Error("blob failed");
  console.log("fetch response body methods passed");
})();
