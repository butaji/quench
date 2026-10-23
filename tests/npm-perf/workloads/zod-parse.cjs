const { z } = require("zod");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);

const flatSchema = z.object({
  id: z.string(),
  label: z.string().min(1),
  count: z.number().int().nonnegative(),
  enabled: z.boolean(),
});
const nestedSchema = z.object({
  user: z.object({ id: z.string().uuid(), name: z.string().min(1) }),
  items: z.array(z.object({ sku: z.string(), quantity: z.number().int().positive() })),
  tags: z.array(z.string()).max(8),
});
const eventSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("created"), id: z.string(), at: z.number().int() }),
  z.object({ type: z.literal("deleted"), id: z.string(), reason: z.string().optional() }),
]);

const flatInput = { id: "record-42", label: "catalog-item", count: 3, enabled: true };
const nestedInput = {
  user: { id: "6ba7b810-9dad-11d1-80b4-00c04fd430c8", name: "Ada" },
  items: [{ sku: "A-1", quantity: 2 }, { sku: "B-7", quantity: 5 }],
  tags: ["new", "featured", "sale"],
};
const eventInput = { type: "created", id: "evt-8", at: 1_800_000_000 };
const invalidInput = { id: "record-42", label: "catalog-item", count: 3.5, enabled: "yes" };

function operations() {
  const flat = flatSchema.parse(flatInput);
  const nested = nestedSchema.parse(nestedInput);
  const event = eventSchema.parse(eventInput);
  const invalid = flatSchema.safeParse(invalidInput);
  if (flat.id !== "record-42" || flat.count !== 3 ||
      nested.user.name !== "Ada" || nested.items.length !== 2 ||
      event.type !== "created" || event.at !== 1_800_000_000 ||
      invalid.success || invalid.error.issues.length !== 2 ||
      invalid.error.issues[0].path[0] !== "count" ||
      invalid.error.issues[1].path[0] !== "enabled") {
    throw new Error("Zod workload result mismatch");
  }
  return flat.id.length + flat.count + nested.items.length + nested.tags.length +
    event.id.length + invalid.error.issues.length;
}

for (let index = 0; index < warmups; index += 1) operations();
let checksum = 0;
const start = Date.now();
for (let index = 0; index < rounds; index += 1) checksum = (checksum + operations()) >>> 0;
const elapsedMs = Math.max(1, Date.now() - start);
const operationsPerRound = 4;
console.log(JSON.stringify({
  workload: "zod-parse",
  package: "zod@4.6.7",
  rounds,
  warmups,
  cases: ["flat-parse", "nested-array-parse", "discriminated-union-parse", "flat-safeParse-invalid"],
  bytes: 0,
  checksum,
  elapsedMs,
  operationsPerSecond: rounds * operationsPerRound * 1000 / elapsedMs,
}));
