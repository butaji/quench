const { NextResponse } = require("next/server");
const { getRouteMatcher } = require("next/dist/shared/lib/router/utils/route-matcher");
const { getRouteRegex } = require("next/dist/shared/lib/router/utils/route-regex");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
const routeDefs = [
  ["/products/[category]/[...slug]", "/products/books/runtime/performance"],
  ["/docs/[...slug]", "/docs/api/server/route-handlers"],
  ["/users/[id]", "/users/8421"],
  ["/api/items/[id]/reviews/[reviewId]", "/api/items/sku-42/reviews/rev-9"],
];
const routes = routeDefs.map(([route, pathname], index) => {
  const match = getRouteMatcher(getRouteRegex(route));
  const params = match(pathname);
  if (!params) throw new Error(`route failed to match ${pathname}`);
  const payload = { route: index, params };
  const encoded = JSON.stringify(payload);
  return {
    index,
    pathname,
    match,
    expectedKeys: Object.keys(params),
    expectedParams: params,
    bytes: Buffer.byteLength(encoded),
    checksum: [...encoded].reduce((sum, character) => (sum + character.charCodeAt(0)) >>> 0, 0),
  };
});

function sameValue(actual, expected) {
  if (Array.isArray(expected)) {
    return Array.isArray(actual) && actual.length === expected.length &&
      actual.every((value, index) => value === expected[index]);
  }
  return actual === expected;
}

function request(index) {
  const route = routes[index % routes.length];
  const params = route.match(route.pathname);
  if (!params || route.expectedKeys.some((key) => !sameValue(params[key], route.expectedParams[key]))) {
    throw new Error(`route params mismatch for ${route.pathname}`);
  }
  const response = NextResponse.json({ route: route.index, params }, { status: 200 });
  if (response.status !== 200 || !response.body ||
    !response.headers.get("content-type")?.startsWith("application/json")) {
    throw new Error("NextResponse output metadata mismatch");
  }
  return route;
}

function main() {
  for (let index = 0; index < warmups; index += 1) request(index);
  let checksum = 0;
  let bytes = 0;
  const start = Date.now();
  for (let index = 0; index < rounds; index += 1) {
    const route = request(index);
    bytes += route.bytes;
    checksum = (checksum + route.checksum) >>> 0;
  }
  const elapsedMs = Math.max(1, Date.now() - start);
  console.log(JSON.stringify({
    workload: "next-route-generate",
    package: "next@16.4.3",
    rounds,
    warmups,
    cases: ["dynamic-route-match", "NextResponse.json", "response-stream-ready"],
    bytes,
    checksum,
    elapsedMs,
    operationsPerSecond: rounds * 1000 / elapsedMs,
  }));
}

try {
  main();
} catch (error) {
  console.error(error);
  process.exitCode = 1;
}
