"use strict";

function readPath(value, path) {
  return path.split(".").reduce((current, key) => current?.[key], value);
}

function rulesFor(contracts, benchmark) {
  const specific = contracts.benchmarks?.[benchmark];
  if (!specific) throw new Error(`missing profile contract for ${benchmark}`);
  return { ...contracts.defaults, ...specific };
}

function violations(report, contracts, benchmark = report.fixture?.replace(/\.js$/, "")) {
  return Object.entries(rulesFor(contracts, benchmark)).flatMap(([path, rule]) => {
    const actual = readPath(report, path);
    if (typeof actual !== "number" || !Number.isFinite(actual)) {
      return [{ path, actual, rule, reason: "missing numeric measurement" }];
    }
    if (rule.min !== undefined && actual < rule.min) {
      return [{ path, actual, rule, reason: `below ${rule.min}` }];
    }
    if (rule.max !== undefined && actual > rule.max) {
      return [{ path, actual, rule, reason: `above ${rule.max}` }];
    }
    return [];
  });
}

function formatViolations(benchmark, failures) {
  const lines = failures.map(({ path, actual, reason }) =>
    `  ${path} = ${actual ?? "missing"}; ${reason}`);
  return [`${benchmark} execution profile failed:`, ...lines].join("\n");
}

module.exports = { formatViolations, readPath, rulesFor, violations };
