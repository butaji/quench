#!/usr/bin/env node
"use strict";

// Audit platform classifications without deciding whether a fixture should be
// implemented.  The audit proves only that a classification is anchored to
// real fixtures, a current differential observation (when a report is given),
// and an explicit rationale.

const fs = require("node:fs");
const path = require("node:path");

const [rootArg, reportPath, ownershipPath, fixtureRootArg] = process.argv.slice(
  2,
);
if (!rootArg || !ownershipPath) {
  console.error(
    "usage: audit-platform-coverage.cjs ROOT [REPORT] OWNERSHIP [FIXTURE_ROOT]",
  );
  process.exit(2);
}

const root = path.resolve(rootArg);
const ownershipFile = path.resolve(ownershipPath);
const fixtureRoot = path.resolve(
  fixtureRootArg || path.join(root, "tests/node/test/parallel"),
);
const defaultFixtureRoot = path.resolve(root, "tests/node/test/parallel");
const fullFixtureSelection = fixtureRoot === defaultFixtureRoot;
const failures = [];
const warnings = [];

function fail(message) {
  failures.push(message);
}

function readJson(file, label) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    fail(`${label}: ${error.message}`);
    return null;
  }
}

function relativeFixture(file) {
  return path.relative(root, path.resolve(file)).split(path.sep).join("/");
}

function fixtureFiles(target) {
  if (!fs.existsSync(target)) {
    fail(`fixture root does not exist: ${relativeFixture(target)}`);
    return [];
  }
  const stat = fs.statSync(target);
  if (stat.isFile()) {
    return /\.(?:js|mjs|cjs)$/.test(target) ? [relativeFixture(target)] : [];
  }
  const files = [];
  for (const entry of fs.readdirSync(target, { withFileTypes: true })) {
    const full = path.join(target, entry.name);
    if (entry.isDirectory()) files.push(...fixtureFiles(full));
    else if (entry.isFile() && /\.(?:js|mjs|cjs)$/.test(entry.name)) {
      files.push(relativeFixture(full));
    }
  }
  return files.sort();
}

// Keep this exactly aligned with diff-node-quench*.sh.  A disagreement here
// would make ownership counts look correct while queueing under the wrong
// prefix.
function derivePrefix(fixture) {
  const fixtureName = path.basename(fixture);
  const prefixName = fixtureName.replace(/^test-/, "");
  const prefix = prefixName.split("-")[0];
  return fixtureName === prefixName || prefix === prefixName
    ? "unprefixed"
    : prefix;
}

function hasReason(reason, label) {
  if (typeof reason !== "string" || reason.trim().length < 20) {
    fail(`${label}: missing evidence rationale (20+ characters required)`);
    return false;
  }
  return true;
}

const ownership = readJson(ownershipFile, "ownership");
const fixtures = fixtureFiles(fixtureRoot);
const fixtureSet = new Set(fixtures);
const fixturePrefixes = new Map();
for (const fixture of fixtures) {
  const prefix = derivePrefix(fixture);
  const list = fixturePrefixes.get(prefix) || [];
  list.push(fixture);
  fixturePrefixes.set(prefix, list);
}

if (ownership) {
  if (!ownership.streams || typeof ownership.streams !== "object") {
    fail("ownership.streams must be an object");
  }
  const prefixOwners = new Map();
  for (const [owner, prefixes] of Object.entries(ownership.streams || {})) {
    if (!Array.isArray(prefixes)) {
      fail(`stream ${owner}: prefixes must be an array`);
      continue;
    }
    for (const prefix of prefixes) {
      if (typeof prefix !== "string" || !prefix) {
        fail(`stream ${owner}: invalid prefix`);
        continue;
      }
      const previous = prefixOwners.get(prefix);
      if (previous) {
        fail(`prefix ${prefix}: owned by both ${previous} and ${owner}`);
      }
      prefixOwners.set(prefix, owner);
    }
  }

  for (
    const [prefix, reason] of Object.entries(ownership.platformLimited || {})
  ) {
    hasReason(reason, `platform prefix ${prefix}`);
    const matches = fixturePrefixes.get(prefix) || [];
    if (!matches.length && fullFixtureSelection) {
      fail(`platform prefix ${prefix}: no fixture in selection`);
    }
  }

  const limitedFixturePatterns = Object.entries(
    ownership.platformLimitedFixtures || {},
  );
  const patternMatches = [];
  for (const [pattern, reason] of limitedFixturePatterns) {
    hasReason(reason, `fixture pattern ${pattern}`);
    const matches = fixtures.filter((fixture) =>
      pattern.endsWith(".js") || pattern.endsWith(".mjs") ||
        pattern.endsWith(".cjs")
        ? fixture.endsWith(pattern)
        : fixture.includes(pattern)
    );
    if (!matches.length) {
      if (fullFixtureSelection) {
        fail(`fixture pattern ${pattern}: no fixture in selection`);
      }
    } else {
      patternMatches.push([pattern, matches]);
    }
  }
  for (let i = 0; i < patternMatches.length; i += 1) {
    for (let j = i + 1; j < patternMatches.length; j += 1) {
      const [left, leftMatches] = patternMatches[i];
      const [right, rightMatches] = patternMatches[j];
      const overlap = leftMatches.filter((fixture) =>
        rightMatches.includes(fixture)
      );
      if (overlap.length) {
        fail(
          `fixture patterns ${left} and ${right} overlap: ${
            overlap.join(", ")
          }`,
        );
      }
    }
  }

  const moduleReasons = ownership.platformLimitedModules || {};
  for (const [name, reason] of Object.entries(moduleReasons)) {
    if (!/^node:/.test(name)) {
      fail(`platform module ${name}: use the node: name`);
    }
    hasReason(reason, `platform module ${name}`);
  }

  let report = null;
  if (reportPath) report = readJson(path.resolve(reportPath), "report");
  if (report) {
    if (report.schema !== 2) {
      fail("report: schema 2 is required for an authoritative audit");
    }
    if (!Array.isArray(report.results)) {
      fail("report: results must be an array");
    } else {
      const results = new Map();
      for (const result of report.results) {
        const fixture = String(result.fixture || "").replace(/\\/g, "/");
        const normalized = fixture.startsWith("/")
          ? relativeFixture(fixture)
          : fixture.replace(/^\.\//, "");
        if (!fixtureSet.has(normalized)) {
          fail(`report fixture is outside selection: ${fixture}`);
        }
        if (results.has(normalized)) {
          fail(`report duplicate fixture: ${normalized}`);
        }
        results.set(normalized, { ...result, fixture: normalized });
        const expectedPrefix = derivePrefix(normalized);
        if (result.prefix !== expectedPrefix) {
          fail(
            `report prefix mismatch for ${normalized}: ${result.prefix} != ${expectedPrefix}`,
          );
        }
      }
      for (const fixture of fixtures) {
        if (!results.has(fixture)) fail(`report missing fixture: ${fixture}`);
      }

      const currentNonMatch = (result) => result.category !== "match";
      const hostAffected = (result) =>
        result.category === "output-mismatch" ||
        ["quench-failed", "both-failed", "timeout"].includes(result.category) &&
          Number(result.quench?.status ?? 0) !== 0;

      for (const [prefix] of Object.entries(ownership.platformLimited || {})) {
        const prefixResults = [...results.values()].filter((result) =>
          result.prefix === prefix
        );
        if (!prefixResults.length && !fullFixtureSelection) continue;
        const failuresForPrefix = prefixResults.filter(currentNonMatch);
        const hostFailures = failuresForPrefix.filter(hostAffected);
        if (!failuresForPrefix.length) {
          warnings.push(`platform prefix ${prefix}: no current non-match`);
        } else if (!hostFailures.length) {
          fail(
            `platform prefix ${prefix}: non-matches have no quench-side failure`,
          );
        }
      }

      for (const [pattern, matches] of patternMatches) {
        const matchingResults = matches.map((fixture) => results.get(fixture))
          .filter(Boolean);
        const nonMatches = matchingResults.filter(currentNonMatch);
        const hostFailures = nonMatches.filter(hostAffected);
        if (!nonMatches.length) {
          warnings.push(`fixture pattern ${pattern}: no current non-match`);
        } else if (!hostFailures.length) {
          fail(`fixture pattern ${pattern}: no quench-side failure`);
        }
      }
    }
  }

  const limitedPrefixes = new Set(Object.keys(ownership.platformLimited || {}));
  const limitedFixtureCount = fixtures.filter((fixture) =>
    limitedPrefixes.has(derivePrefix(fixture))
  ).length;
  console.log(`fixtures=${fixtures.length}`);
  console.log(`platform_prefixes=${limitedPrefixes.size}`);
  console.log(`platform_prefix_fixtures=${limitedFixtureCount}`);
  console.log(
    `platform_fixture_patterns=${
      Object.keys(ownership.platformLimitedFixtures || {}).length
    }`,
  );
  console.log(
    `report=${
      reportPath
        ? path.relative(root, path.resolve(reportPath))
        : "not supplied"
    }`,
  );
  if (warnings.length) {
    console.log(`warnings=${warnings.length}`);
    for (const warning of warnings) console.log(`WARNING\t${warning}`);
  }
}

if (failures.length) {
  console.error(`platform coverage audit failed (${failures.length})`);
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}
console.log("platform coverage audit passed");
