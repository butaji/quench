#!/usr/bin/env python3
"""Per-symbol diff for Ink parity harness.

A symbol is a whitespace-separated token or a box-drawing character.
Similarity is computed via LCS (longest common subsequence).
"""

import sys
import json

BOX_DRAWING = set(
    "│─└┌┐┘├┤┬┴┼═║╔╗╚╝╠╣╦╩╬"
)


def tokenize(text: str) -> list[str]:
    symbols = []
    for line in text.splitlines():
        line = line.rstrip()
        i = 0
        while i < len(line):
            c = line[i]
            if c in BOX_DRAWING:
                symbols.append(c)
                i += 1
            elif c.isspace():
                i += 1
            else:
                j = i
                while (
                    j < len(line)
                    and line[j] not in BOX_DRAWING
                    and not line[j].isspace()
                ):
                    j += 1
                symbols.append(line[i:j])
                i = j
    return symbols


def lcs_len(a: list[str], b: list[str]) -> int:
    m, n = len(a), len(b)
    if m == 0 or n == 0:
        return 0
    prev = [0] * (n + 1)
    curr = [0] * (n + 1)
    for i in range(1, m + 1):
        for j in range(1, n + 1):
            if a[i - 1] == b[j - 1]:
                curr[j] = prev[j - 1] + 1
            else:
                curr[j] = max(prev[j], curr[j - 1])
        prev, curr = curr, prev
    return prev[n]


def similarity(text1: str, text2: str) -> float:
    s1 = tokenize(text1)
    s2 = tokenize(text2)
    if len(s1) == 0 and len(s2) == 0:
        return 100.0
    if len(s1) == 0 or len(s2) == 0:
        return 0.0
    common = lcs_len(s1, s2)
    return 2.0 * common / (len(s1) + len(s2)) * 100.0


def main() -> int:
    if len(sys.argv) == 3:
        with open(sys.argv[1], "r", encoding="utf-8") as f:
            t1 = f.read()
        with open(sys.argv[2], "r", encoding="utf-8") as f:
            t2 = f.read()
        print(f"{similarity(t1, t2):.2f}")
        return 0
    # JSON mode from stdin
    data = json.load(sys.stdin)
    t1 = data.get("a", "")
    t2 = data.get("b", "")
    result = similarity(t1, t2)
    print(json.dumps({"similarity": round(result, 2)}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
