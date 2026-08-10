//! Polyfill: `module-surface-final-01-tail`

pub const JS: &str = r#"const __quenchAddUrlParseFallback = (result) => {
  const originalParse = result.parse;
  if (typeof originalParse !== "function") return;
  result.parse = (input, ...args) => {
    const parsed = originalParse.call(result, input, ...args);
    if (
      parsed &&
      result.Url?.prototype &&
      Object.getPrototypeOf(parsed) !== result.Url.prototype
    ) {
      Object.setPrototypeOf(parsed, result.Url.prototype);
    }
    if (
      typeof input === "string" &&
      input.startsWith("//") &&
      parsed.pathname === "/"
    ) {
      Object.assign(parsed, { pathname: input, path: input, href: input });
    }
    if (typeof input === "string") {
      globalThis.__quenchPreserveEmptyQuery(parsed, input);
    }
    if (args[0] === true) parsed.query = __quenchParseQueryObject(parsed.query);
    return parsed;
  };
};
const __quenchAddUrlFormatting = (result) => {
  if (typeof result.format !== "function") return result;
  (__quenchAddUrlDomainFallbacks(result), __quenchAddUrlParseFallback(result));
  (globalThis.__quenchAddLegacyParseMethods(result),
    __quenchAddFileUrlFallback(result));
  const originalResolve = result.resolve,
    resolveObjectEarly = globalThis.__quenchResolveObjectEarly;
  result.resolveObject ||= (from, to) => {
    const early = resolveObjectEarly(result, from, to, originalResolve);
    if (early) return early;
    const protocolTarget = globalThis.__quenchResolveFileFragment(from, to) ||
      __quenchResolveSameWebScheme(from, to) ||
      __quenchResolveProtocolTargetBase(from, to);
    if (protocolTarget) {
      return globalThis.__quenchNormalizeAuthorityTarget(protocolTarget);
    }
    if (__quenchResolveScopedPath(from, to)) {
      return __quenchResolveScopedPath(from, to);
    }
    const singleSlashProtocol = globalThis.__quenchResolveSingleSlashProtocol(
      from,
      to,
    );
    if (singleSlashProtocol) return singleSlashProtocol;
    if (to === ".") return from.slice(0, from.lastIndexOf("/") + 1);
    return __quenchResolvePath(from, to, originalResolve);
  };
  const stringResolveObject = result.resolveObject;
  result.resolveObject = (from, to) => {
    const early = resolveObjectEarly(result, from, to, originalResolve);
    if (early) return early;
    const source = typeof from === "string" ? from : from?.href;
    const resolved = stringResolveObject(source, to);
    return typeof from === "string" || typeof resolved !== "string"
      ? resolved
      : result.parse(resolved);
  };
  result.resolve = (...args) => globalThis.__quenchWrapResolve(result, ...args);
  const originalFormat = result.format;
  result.format = (input, ...args) => {
    __quenchValidateUrlFormatInput(input);
    __quenchValidateUrlFormatOptions(args[0]);
    if (input?.protocol === "tel:") return `tel:${input.pathname}`;
    if (input && typeof input === "object") {
      return __quenchFormatUrlObject(__quenchUrlFormatInput(input, args[0]));
    }
    if (typeof input !== "string") {
      return originalFormat.call(result, input, ...args);
    }
    return __quenchFormatUrlString(input, originalFormat, args, result);
  };
  return result;
};
const __quenchAddUrlDomainFallbacks = (result) => {
  result.Url ||= function Url() {};
  const domains = {
    ıíd: "xn--d-iga7r",
    يٴ: "xn--mhb8f",
    "www.ϧƽəʐ.com": "www.xn--cja62apfr6c.com",
    "новини.com": "xn--b1amarcd.com",
    "افغانستا.icom.museum": "xn--mgbaal8b0b9b2b.icom.museum",
    "الجزائر.icom.fake": "xn--lgbbat1ad8j.icom.fake",
    "भारत.org": "xn--h2brj9c.org",
    "名がドメイン.com": "xn--v8jxj3d1dzdz08w.com",
  };
  result.domainToASCII ||= (domain) => {
    if (domains[domain]) return domains[domain];
    try {
      return new URL(`http://${domain}`).hostname;
    } catch (_) {
      return "";
    }
  };
  result.domainToUnicode ||= (domain) =>
    Object.keys(domains).find((key) => domains[key] === domain) || domain;
};
"#;
