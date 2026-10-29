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
    // Headers methods whose name and value undici's steps would keep as they are skip those
    // steps (`modules::webidl_shared_vm::header_fields`); any other call runs them.
    let mut source = source;
    for (from, fast) in HEADER_FAST_PATHS {
        source = replace_node_source_once(&source, from, &format!("{fast}{from}"));
    }
    Some(source)
}

/// Each patched Headers step, its original first steps, and the fast path put before them:
/// with `header_fields_binding!()` accepting its arguments unchanged, it does the steps that
/// follow their checks (the guard, then the header list), and otherwise falls through.
const HEADER_FAST_PATHS: [(&str, &str); 6] = [
    (
        "      value = headerValueNormalize(value);\n      if (!isValidHeaderName(name)) {",
        concat!(
            "      if (", header_fields_binding!(), "(name, value)) {\n",
            "        if (getHeadersGuard(headers) === \"immutable\") {\n          throw new TypeError(\"immutable\");\n        }\n",
            "        return getHeadersList(headers).append(name, value, false);\n      }\n",
        ),
    ),
    (
        "        webidl.argumentLengthCheck(arguments, 2, \"Headers.append\");",
        concat!(
            "        if (arguments.length >= 2 && ", header_fields_binding!(), "(name, value)) {\n",
            "          if (this.#guard === \"immutable\") {\n            throw new TypeError(\"immutable\");\n          }\n",
            "          return this.#headersList.append(name, value, false);\n        }\n",
        ),
    ),
    (
        "        webidl.argumentLengthCheck(arguments, 1, \"Headers.delete\");",
        concat!(
            "        if (arguments.length >= 1 && ", header_fields_binding!(), "(name)) {\n",
            "          if (this.#guard === \"immutable\") {\n            throw new TypeError(\"immutable\");\n          }\n",
            "          if (this.#headersList.contains(name, false)) {\n            this.#headersList.delete(name, false);\n          }\n",
            "          return;\n        }\n",
        ),
    ),
    (
        "        webidl.argumentLengthCheck(arguments, 1, \"Headers.get\");",
        concat!(
            "        if (arguments.length >= 1 && ", header_fields_binding!(), "(name)) {\n",
            "          return this.#headersList.get(name, false);\n        }\n",
        ),
    ),
    (
        "        webidl.argumentLengthCheck(arguments, 1, \"Headers.has\");",
        concat!(
            "        if (arguments.length >= 1 && ", header_fields_binding!(), "(name)) {\n",
            "          return this.#headersList.contains(name, false);\n        }\n",
        ),
    ),
    (
        "        webidl.argumentLengthCheck(arguments, 2, \"Headers.set\");",
        concat!(
            "        if (arguments.length >= 2 && ", header_fields_binding!(), "(name, value)) {\n",
            "          if (this.#guard === \"immutable\") {\n            throw new TypeError(\"immutable\");\n          }\n",
            "          this.#headersList.set(name, value, false);\n          return;\n        }\n",
        ),
    ),
];
