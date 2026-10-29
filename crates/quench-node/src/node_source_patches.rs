fn replace_node_source_once(source: &str, from: &str, to: &str) -> String {
    let count = source.matches(from).count();
    assert_eq!(
        count, 1,
        "pinned Node source patch matched {count} locations"
    );
    source.replacen(from, to, 1)
}

fn patch_node_source(relative: &str, source: &str) -> Option<String> {
    if relative != "deps/undici/undici.js" {
        return None;
    }

    let source = replace_node_source_once(
        source,
        "const body = extractBody(bytes);",
        "const body = extractBody(bytes, false, true);",
    );
    let source = replace_node_source_once(
        &source,
        "    function extractBody(object, keepalive = false) {\n      let stream = null;\n      let controller = null;",
        r#"    function extractBody(object, keepalive = false, jsonResponse = false) {
      if (jsonResponse && typeof object?.[Symbol.asyncIterator] !== "function") {
        const source = webidl.util.getCopyOfBytesHeldByBufferSource(object);
        let controller = null;
        const stream = new ReadableStream({
          start(c) {
            controller = c;
          },
          type: "bytes"
        });
        assert(webidl.is.ReadableStream(stream));
        if (source.length) {
          controller.enqueue(new Uint8Array(source));
        }
        queueMicrotask(() => readableStreamClose(controller));
        return [{ stream, source, length: source.length }, null];
      }
      let stream = null;
      let controller = null;"#,
    );
    let source = replace_node_source_once(
        &source,
        "        const body = extractBody(bytes, false, true);\n        const responseObject = fromInnerResponse(makeResponse({}), \"response\");\n        initializeResponse(responseObject, init, { body: body[0], type: \"application/json\" });\n        return responseObject;",
        r#"        const body = extractBody(bytes, false, true);
        if (init.headers == null && init.statusText === "") {
          if (init.status < 200 || init.status > 599) {
            throw new RangeError('init["status"] must be in the range of 200 to 599, inclusive.');
          }
          if (nullBodyStatus.includes(init.status)) {
            throw webidl.errors.exception({
              header: "Response constructor",
              message: `Invalid response status code ${init.status}`
            });
          }
          const responseObject = fromInnerResponse(makeResponse({}), "response");
          const state = getResponseState(responseObject);
          state.status = init.status;
          state.headersList.append("content-type", "application/json", true);
          state.body = body[0];
          return responseObject;
        }
        const responseObject = fromInnerResponse(makeResponse({}), "response");
        initializeResponse(responseObject, init, { body: body[0], type: "application/json" });
        return responseObject;"#,
    );
    // WebIDL ByteString's scan starts where a code unit above 255 is, or past the end
    // (`modules::webidl_shared_vm::byte_string_end`).
    let source = replace_node_source_once(
        &source,
        "      const x = String(V);\n      for (let index = 0; index < x.length; index++) {\n        if (x.charCodeAt(index) > 255) {",
        concat!(
            "      const x = String(V);\n      for (let index = ",
            byte_string_end_binding!(),
            "(x); index < x.length; index++) {\n        if (x.charCodeAt(index) > 255) {"
        ),
    );
    Some(source)
}
