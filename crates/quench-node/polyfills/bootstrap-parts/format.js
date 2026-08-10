const __nodeLegacyUrlEncodePath = (value) =>
  value
    .replace(/%/g, "~QUENCH_PERCENT~")
    .replace(/["\s]/g, (character) => encodeURIComponent(character));
const __nodeLegacyUrlRestorePercent = (value) =>
  value
    .replace(/~QUENCH_PERCENT~/g, "%")
    .replace(/%7EQUENCH_PERCENT%7E/gi, "%")
    .replace(/%25/g, "%");

const __nodeLegacyUrlFormatString = (value) => {
  try {
    const hashIndex = value.indexOf("#");
    const hash = hashIndex >= 0 ? value.slice(hashIndex) : "";
    const withoutHash = hashIndex >= 0 ? value.slice(0, hashIndex) : value;
    const queryIndex = withoutHash.indexOf("?");
    const rawQuery = queryIndex >= 0 ? withoutHash.slice(queryIndex + 1) : null;
    const href = rawQuery === null
      ? `${
        __nodeLegacyUrlRestorePercent(
          new globalThis.__nodeURL(__nodeLegacyUrlEncodePath(withoutHash))
            .href,
        )
      }${hash}`
      : (() => {
        const parsed = new globalThis.__nodeURL(
          __nodeLegacyUrlEncodePath(withoutHash.slice(0, queryIndex)),
        );
        return `${
          __nodeLegacyUrlRestorePercent(
            parsed.origin + parsed.pathname,
          )
        }?${rawQuery}${hash}`;
      })();
    const withTrailingSlash = value.endsWith("/") && !href.endsWith("/")
      ? `${href}/`
      : href;
    return value.endsWith("?") && !withTrailingSlash.endsWith("?")
      ? `${withTrailingSlash}?`
      : withTrailingSlash;
  } catch (_) {
    return value;
  }
};
