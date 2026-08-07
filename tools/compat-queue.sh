#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "${1:-}" ]; then
  input=$1
else
  input="$root/target/compat/differential.json"
  if [ -f "$root/target/compat/differential-parallel.json" ]; then
    input="$root/target/compat/differential-parallel.json"
  fi
fi
limit=${2:-25}
previous=${3:-}
ownership="$root/tools/compat-ownership.json"

if [ ! -f "$input" ]; then
  echo "differential report does not exist: $input" >&2
  exit 2
fi

# Never prioritize from a report whose fixture/source inventory is stale. This
# keeps queue output auditable when the Node submodule, polyfills, or focused
# contracts changed since the differential run.
if [ "${QUENCH_COMPAT_ALLOW_STALE:-0}" != "1" ]; then
  "$root/tools/compat-report-status.sh" "$input" "$root/tests/node/test/parallel"
fi

QUENCH_COMPAT_OWNERSHIP="$ownership" node - "$input" "$limit" "$previous" <<'NODE'
const fs = require("fs");

const [, , input, rawLimit, previousPath] = process.argv;
const limit = Number(rawLimit);
const report = JSON.parse(fs.readFileSync(input, "utf8"));
const ownership = JSON.parse(fs.readFileSync(process.env.QUENCH_COMPAT_OWNERSHIP, "utf8"));
const previous = previousPath ? JSON.parse(fs.readFileSync(previousPath, "utf8")) : null;
const streamByPrefix = new Map(Object.entries(ownership.streams).flatMap(([stream, prefixes]) => prefixes.map((prefix) => [prefix, stream])));
const classify = (result) => {
  const fixtureReason = ownership.platformLimitedFixtures
    ? Object.entries(ownership.platformLimitedFixtures).find(([name]) =>
        name.endsWith(".js") || name.endsWith(".mjs")
          ? result.fixture.endsWith(name)
          : result.fixture.includes(name)
      )?.[1]
    : undefined;
  const prefixReason = ownership.platformLimited[result.prefix];
  return {
    owner: streamByPrefix.get(result.prefix) ?? ownership.default.owner,
    status: fixtureReason || prefixReason
      ? "platform-limited"
      : (streamByPrefix.has(result.prefix) ? "owned" : ownership.default.status),
    reason: fixtureReason || prefixReason || ownership.default.reason,
  };
};
const groups = new Map();
const signatureVariants = new Map();

for (const result of report.results) {
  if (result.category === "match") continue;
  const classification = classify(result);
  const variant = `${result.signature}\u0000${result.prefix}\u0000${classification.owner}\u0000${classification.status}\u0000${result.category}`;
  const variants = signatureVariants.get(result.signature) ?? new Set();
  variants.add(variant);
  signatureVariants.set(result.signature, variants);
  const entry = groups.get(variant) ?? {
    signature: result.signature,
    category: result.category,
    classification,
    prefix: result.prefix,
    fixtures: [],
  };
  entry.fixtures.push(result.fixture);
  groups.set(variant, entry);
}

const queue = [...groups.values()]
  .sort((a, b) => {
    // Actionable owned/unclassified work should lead the queue; platform
    // limits remain visible but must not starve implementable API clusters.
    const rank = { owned: 0, unclassified: 1, "platform-limited": 2 };
    return (rank[a.classification.status] ?? 3) -
      (rank[b.classification.status] ?? 3) ||
      b.fixtures.length - a.fixtures.length ||
      a.signature.localeCompare(b.signature);
  })
  .slice(0, limit);

const nonMatches = report.results.filter((result) => result.category !== "match").length;
const conflictSignatures = [...signatureVariants.values()].filter((variants) => variants.size > 1);
console.log(`signatures=${groups.size}`);
console.log(`legacy_signatures=${signatureVariants.size}`);
console.log(`classification_conflict_signatures=${conflictSignatures.length}`);
console.log(`classification_conflict_fixtures=${[...signatureVariants.entries()].filter(([, variants]) => variants.size > 1).reduce((sum, [signature]) => sum + report.results.filter((result) => result.signature === signature && result.category !== "match").length, 0)}`);
console.log(`total_nonmatching_fixtures=${nonMatches}`);
console.log(`displayed_queue_fixtures=${queue.reduce((sum, item) => sum + item.fixtures.length, 0)}`);
const classified = report.results.filter((result) => result.category !== "match").map(classify);
console.log(`platform_limited=${classified.filter((item) => item.status === "platform-limited").length}`);
console.log(`unclassified=${classified.filter((item) => item.status === "unclassified").length}`);
if (previous) {
  const old = new Map(previous.results.map((result) => [result.fixture, result]));
  const regressions = report.results.filter((result) => result.category !== "match" && old.get(result.fixture)?.category === "match");
  const resolved = report.results.filter((result) => result.category === "match" && old.get(result.fixture)?.category !== "match");
  console.log(`regressions=${regressions.length}`);
  console.log(`resolved=${resolved.length}`);
  for (const result of regressions.slice(0, 10)) console.log(`REGRESSION\t${result.prefix}\t${result.fixture}\t${result.signature}`);
  for (const result of resolved.slice(0, 10)) console.log(`RESOLVED\t${result.prefix}\t${result.fixture}`);
}
for (const [index, item] of queue.entries()) {
  const prefix = item.prefix ?? "unknown";
  const classification = item.classification ?? ownership.default;
  console.log(`${index + 1}\t${item.fixtures.length}\t${prefix}\t${classification.owner}\t${classification.status}\t${item.category}\t${item.signature}`);
  for (const fixture of item.fixtures.slice(0, 3)) console.log(`\t${fixture}`);
}
NODE
