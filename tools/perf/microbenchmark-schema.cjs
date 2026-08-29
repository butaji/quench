"use strict";

const ITERATION_ERROR = "iterations must be a positive safe integer";
const COUNTER_VERSION = 1;
// This is a fixed semantic vocabulary, deliberately not an event map. A
// missing counters object means diagnostics were compiled out; a null member
// means that particular counter was unavailable in this diagnostic build.
const COUNTER_FIELDS = Object.freeze([
  "value_decodes",
  "value_clones",
  "value_drops",
  "environment_allocations",
  "generic_calls",
  "fallbacks",
  "property_lookups",
  "array_materializations",
  "allocated_bytes",
]);

function assertIterations(iterations) {
  if (!Number.isSafeInteger(iterations) || iterations < 1) {
    throw new Error(ITERATION_ERROR);
  }
}

function validateCounters(counters) {
  if (counters === undefined) return;
  if (!counters || typeof counters !== "object" || counters.version !== COUNTER_VERSION) {
    throw new TypeError(`benchmark counters require version ${COUNTER_VERSION}`);
  }
  for (const field of COUNTER_FIELDS) {
    const value = counters[field];
    if (value !== null && (!Number.isSafeInteger(value) || value < 0)) {
      throw new TypeError(`benchmark counter ${field} must be a non-negative safe integer or null`);
    }
  }
  for (const field of Object.keys(counters)) {
    if (field !== "version" && !COUNTER_FIELDS.includes(field)) {
      throw new TypeError(`unknown benchmark counter ${field}`);
    }
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
    validateCounters(result.counters);
  }
  return report;
}

module.exports = { COUNTER_FIELDS, COUNTER_VERSION, assertIterations, validateBenchmarkReport, validateCounters };
