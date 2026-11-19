#!/usr/bin/env python3
"""Generates validatorjs-data.h for the validatorjs C analogue.

Inputs (from the JetStream validatorjs row):
  - dist/bundle.es6.js.map: the source map of the validator.js 13.15 build, whose
    sourcesContent holds the library sources (node_modules/validator/lib/...) and
    src/test.mjs (the test suite).
  - the test suite is run once in node with a recording test() (see the comment in
    validatorjs.c) to dump every test() record: validator name, args, valid and
    invalid values. That dump is passed as the second argument (JSON).

Output: validatorjs-data.h with
  - CASE_JSON: one entry per assertion: validator index, input (JSON text), args (JSON
    text) and the expected result (valid list = 1, invalid list = 0);
  - RX_DEFS: every regular expression literal of the library, named "<file>.<name>" or
    "<file>.<table>[<key>]" (object-table entries and their aliases), as JS source and
    flags, C-escaped.

Usage: validatorjs-gen.py <bundle.es6.js.map> <records.json> <output.h> [alpha-tables.json]
"""
import json
import re
import sys


def lib_sources(map_path):
    m = json.load(open(map_path, encoding="utf-8"))
    out = {}
    for name, src in zip(m["sources"], m["sourcesContent"]):
        if src is None or "/node_modules/validator/lib/" not in name:
            continue
        rel = name.split("/node_modules/validator/lib/")[1]
        out[rel] = src
    return out


def strip_comments_and_strings_mask(src):
    """Returns src with comments blanked out (strings kept, for name lookup)."""
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c in "'\"`":
            q = c
            i += 1
            while i < n and src[i] != q:
                if src[i] == "\\":
                    i += 1
                i += 1
            i += 1
            continue
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                out[k] = " "
            i = j
            continue
        if src.startswith("/*", i):
            j = src.find("*/", i + 2)
            j = n if j < 0 else j + 2
            for k in range(i, j):
                if out[k] != "\n":
                    out[k] = " "
            i = j
            continue
        i += 1
    return "".join(out)


REGEX_PREV = set("=:(,[!?&|{;}") | {"return"}


def find_regex_literals(src):
    """Yields (start, end, pattern, flags) for regex literals in code."""
    i, n = 0, len(src)
    last_sig = ""
    res = []
    while i < n:
        c = src[i]
        if c in " \t\r\n":
            i += 1
            continue
        if c in "'\"`":
            q = c
            i += 1
            while i < n and src[i] != q:
                if src[i] == "\\":
                    i += 1
                i += 1
            i += 1
            last_sig = "s"
            continue
        if src.startswith("//", i):
            j = src.find("\n", i)
            i = n if j < 0 else j
            continue
        if src.startswith("/*", i):
            j = src.find("*/", i + 2)
            i = n if j < 0 else j + 2
            continue
        if c == "/" and (last_sig == "" or last_sig in REGEX_PREV or last_sig.endswith("return")):
            start = i
            i += 1
            in_class = False
            body = []
            while i < n:
                ch = src[i]
                if ch == "\\":
                    body.append(src[i:i + 2])
                    i += 2
                    continue
                if ch == "[":
                    in_class = True
                elif ch == "]":
                    in_class = False
                elif ch == "/" and not in_class:
                    break
                body.append(ch)
                i += 1
            pat = "".join(body)
            i += 1
            fm = re.match(r"[a-z]*", src[i:])
            flags = fm.group(0)
            i += len(flags)
            res.append((start, i, pat, flags))
            last_sig = "r"
            continue
        if re.match(r"[A-Za-z_$]", c):
            m = re.match(r"[A-Za-z_$][\w$]*", src[i:])
            word = m.group(0)
            i += len(word)
            last_sig = word if word == "return" else "w"
            continue
        last_sig = c
        i += 1
    return res


def table_name_before(masked, pos):
    """Name of the object table or variable the literal at pos belongs to."""
    head = masked[:pos]
    # nearest assignment "NAME = {" or "NAME = exports.X = {" that is still open
    best = None
    for m in re.finditer(r"(\w+)\s*=\s*(?:exports\.\w+\s*=\s*)*\{", head):
        best = m.group(1)
    # innermost open brace check: the table is "open" if no "};" follows before pos
    if best:
        tail_m = re.search(r"\}\s*;", head[head.rfind(best):])
        if tail_m is None:
            return best
    var = None
    for m in re.finditer(r"(?:var|let|const)\s+(\w+)\s*=", head):
        var = m.group(1)
    return var


def c_escape(s):
    out = []
    for b in s.encode("utf-8"):
        ch = chr(b)
        if ch == '"' or ch == "\\":
            out.append("\\" + ch)
        elif 0x20 <= b < 0x7F:
            out.append(ch)
        else:
            out.append("\\%03o" % b)
    return '"' + "".join(out) + '"'


def gen_regex_defs(libs):
    defs = []
    seen = set()
    for rel in sorted(libs):
        src = libs[rel]
        base = rel.replace(".js", "").replace("/", ".")
        masked = strip_comments_and_strings_mask(src)
        table_entries = {}
        for start, end, pat, flags in find_regex_literals(masked):
            # the original (unmasked) text is needed for the pattern itself
            pat_text = src[start + 1:end].rsplit("/", 1)[0]
            owner = table_name_before(masked, start)
            # key of an object entry: 'key': /re/
            km = re.search(r"(['\"])([^'\"]+)\1\s*:\s*$", masked[:start])
            if not km:
                km = re.search(r"(?<![\w$.])([A-Za-z_$][\w$]*)\s*:\s*$", masked[:start])
                if km:
                    km = (None, km.group(1))
            if km and owner:
                name = "%s.%s[%s]" % (base, owner, km[1] if isinstance(km, tuple) else km.group(2))
            elif owner:
                name = "%s.%s" % (base, owner)
            else:
                continue
            if name in seen:
                k = 2
                while "%s#%d" % (name, k) in seen:
                    k += 1
                name = "%s#%d" % (name, k)
            seen.add(name)
            defs.append((name, pat_text, flags))
        # table entries whose value is a named regex (KEY: fourDigit,) take that regex
        for m in re.finditer(r"^\s*(?:'([^']+)'|([A-Za-z_$][\w$]*))\s*:\s*([A-Za-z_$][\w$]*)\s*,?\s*(?://[^\n]*)?$", masked, re.M):
            key = m.group(1) or m.group(2)
            ident = m.group(3)
            owner = table_name_before(masked, m.start())
            src_def = [d for d in defs if d[0] == "%s.%s" % (base, ident)]
            if owner and src_def:
                nm = "%s.%s[%s]" % (base, owner, key)
                if nm not in seen:
                    seen.add(nm)
                    defs.append((nm, src_def[0][1], src_def[0][2]))
        for m in re.finditer(r"(\w+)\['([^']+)'\]\s*=\s*\1\['([^']+)'\]\s*;", masked):
            owner, key, other = m.group(1), m.group(2), m.group(3)
            src_def = [d for d in defs if d[0] == "%s.%s[%s]" % (base, owner, other)]
            if src_def:
                defs.append(("%s.%s[%s]" % (base, owner, key), src_def[0][1], src_def[0][2]))
    return defs


def main():
    map_path, records_path, out_path = sys.argv[1:4]
    libs = lib_sources(map_path)
    records = json.load(open(records_path, encoding="utf-8"))["records"]
    names = []
    for r in records:
        if r["validator"] not in names:
            names.append(r["validator"])
    lines = []
    lines.append("/* Generated by validatorjs-gen.py from the validator.js 13.15 sources and test suite.\n"
                 "   Do not edit by hand. */")
    lines.append("static const char *const VJ_FN_NAMES[] = {")
    for nm in names:
        lines.append("  \"%s\"," % nm)
    lines.append("};")
    lines.append("#define VJ_NFN %d" % len(names))
    lines.append("static const struct { int fn; const char *input; const char *args; int expected; } VJ_CASES[] = {")
    total = 0
    for r in records:
        fi = names.index(r["validator"])
        args = json.dumps(r.get("args", []), separators=(",", ":"))
        for key, exp in (("valid", 1), ("invalid", 0)):
            for v in (r.get(key) or []):
                inp = json.dumps(v, separators=(",", ":"))
                lines.append("  {%d, %s, %s, %d}," % (fi, c_escape(inp), c_escape(args), exp))
                total += 1
    lines.append("};")
    lines.append("#define VJ_NCASES %d" % total)
    defs = gen_regex_defs(libs)
    # The locale tables of alpha.js get their aliases from loops, which the source scan does not
    # follow: the tables as validator.js evaluates them (alpha-tables.json, from dump-alpha.cjs)
    # add the missing entries.
    rx_names = set(d[0] for d in defs)
    if len(sys.argv) > 4:
        tables = json.load(open(sys.argv[4], encoding="utf-8"))
        for t in ("alpha", "alphanumeric"):
            for key, rx in tables[t].items():
                name = "alpha.%s[%s]" % (t, key)
                if name not in rx_names:
                    defs.append((name, rx["src"], rx["flags"]))
                    rx_names.add(name)
        dec = tables["decimal"]
    else:
        dec = {}
    lines.append("static const struct { const char *name, *src, *flags; } VJ_RX[] = {")
    for name, pat, flags in defs:
        lines.append("  {%s, %s, %s}," % (c_escape(name), c_escape(pat), c_escape(flags)))
    lines.append("};")
    lines.append("#define VJ_NRX %d" % len(defs))
    lines.append("static const struct { const char *locale, *sep; } VJ_DEC[] = {")
    for key in sorted(dec):
        lines.append("  {%s, %s}," % (c_escape(key), c_escape(dec[key])))
    lines.append("};")
    lines.append("#define VJ_NDEC %d" % len(dec))
    open(out_path, "w", encoding="utf-8").write("\n".join(lines) + "\n")
    print("cases", total, "functions", len(names), "regex defs", len(defs))


if __name__ == "__main__":
    main()
