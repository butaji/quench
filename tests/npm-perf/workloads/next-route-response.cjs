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
const routes = routeDefs.map(([route, pathname], index) => ({
  index,
  pathname,
  match: getRouteMatcher(getRouteRegex(route)),
}));

async function request(index) {
  const route = routes[index % routes.length];
  const params = route.match(route.pathname);
  if (!params) throw new Error(`route failed to match ${route.pathname}`);
  const response = NextResponse.json({ route: route.index, params }, { status: 200 });
  if (response.status !== 200 || !response.headers.get("content-type")?.startsWith("application/json")) {
    throw new Error("NextResponse metadata mismatch");
  }
  const body = await response.json();
  if (body.route !== route.index || JSON.stringify(body.params) !== JSON.stringify(params)) {
    throw new Error("Next route/response result mismatch");
  }
  return JSON.stringify(body);
}

async function main() {
  for (let index = 0; index < warmups; index += 1) await request(index);
  let checksum = 0;
  let bytes = 0;
  const start = Date.now();
  for (let index = 0; index < rounds; index += 1) {
    const body = await request(index);
    bytes += Buffer.byteLength(body);
    checksum = (checksum + body.charCodeAt(index % body.length)) >>> 0;
  }
  const elapsedMs = Math.max(1, Date.now() - start);
  console.log(JSON.stringify({
    workload: "next-route-response",
    package: "next@16.4.3",
    rounds,
    warmups,
    cases: ["dynamic-route-match", "NextResponse.json", "Response.json"],
    bytes,
    checksum,
    elapsedMs,
    operationsPerSecond: rounds * 1000 / elapsedMs,
  }));
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
