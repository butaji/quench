"use strict";

const ITERATION_ERROR = "iterations must be a positive safe integer";

function assertIterations(iterations) {
  if (!Number.isSafeInteger(iterations) || iterations < 1) {
    throw new Error(ITERATION_ERROR);
  }
}

/**
 * Canonical report contract: ownership stays with the producer until the
 * report is serialized; consumers only inspect immutable scalar observations.
 * A report is invalid when it has no samples, duplicate names, or a non-finite
 * measurement. `wall_ms` is intentionally excluded from semantic equality by
 * callers because it is an environment-dependent observation.
 */
function validateBenchmarkReport(report, expectedIterations) {
  if (!report || typeof report !== "object" || !Array.isArray(report.results) || report.results.length === 0) {
    throw new TypeError("benchmark report must contain non-empty results");
  }
  const names = new Set();
  for (const result of report.results) {
    if (!result || typeof result !== "object" || typeof result.name !== "string" || result.name.length === 0) {
      throw new TypeError("benchmark result requires a name");
    }
    if (names.has(result.name)) throw new TypeError("benchmark result names must be unique");
    names.add(result.name);
    assertIterations(result.iterations);
    if (expectedIterations !== undefined && result.iterations !== expectedIterations) {
      throw new TypeError("benchmark result iterations mismatch");
    }
    if (typeof result.checksum !== "number" || !Number.isFinite(result.checksum)) {
      throw new TypeError("benchmark result checksum must be finite");
    }
    if (typeof result.wall_ms !== "number" || !Number.isFinite(result.wall_ms) || result.wall_ms < 0) {
      throw new TypeError("benchmark result wall_ms must be non-negative and finite");
    }
  }
  return report;
}

module.exports = { assertIterations, validateBenchmarkReport };
