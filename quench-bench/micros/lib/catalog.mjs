import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import crypto from "node:crypto";
import { fileURLToPath } from "node:url";

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const hash = (value) => crypto.createHash("sha256").update(value).digest("hex");

function readCase(file) {
  const source = fs.readFileSync(path.join(ROOT, file), "utf8");
  const registrations = [];
  vm.runInNewContext(source, { registerMicro: (c) => registrations.push(c) }, { timeout: 1000, filename: file });
  if (registrations.length !== 1) throw new Error(`${file}: expected one registration`);
  const c = registrations[0];
  const variants = validateCase(c, file);
  return { ...c, variants, file, source, hash: hash(source), legacy: false };
}

function validateCase(c, file) {
  if (!/^[a-z][a-z0-9-]+$/.test(c.id) || !c.question || typeof c.setup !== "function") throw new Error(`${file}: invalid case`);
  for (const field of ["requires", "axes", "observations", "explanations"]) {
    if (!Array.isArray(c[field])) throw new Error(`${file}: missing ${field}`);
  }
  const variants = Object.keys(c.variants || {});
  if (variants.length < 2 || variants.some((v) => typeof c.variants[v] !== "function")) throw new Error(`${file}: contrasts missing`);
  validateEquivalence(c, variants, file);
  return variants;
}

function validateEquivalence(c, variants, file) {
  for (const group of c.equivalent || []) {
    if (group.length < 2 || group.some((v) => !variants.includes(v))) throw new Error(`${file}: invalid equivalence`);
  }
}

export function validateGraph(cases) {
  const byId = new Map(cases.map((c) => [c.id, c]));
  if (byId.size !== cases.length) throw new Error("duplicate experiment ID");
  const done = new Set(), active = new Set();
  function visit(id) {
    if (active.has(id)) throw new Error(`dependency cycle: ${id}`);
    if (done.has(id)) return;
    const c = byId.get(id);
    if (!c) throw new Error(`missing prerequisite: ${id}`);
    active.add(id);
    for (const parent of c.requires) visit(parent);
    active.delete(id);
    done.add(id);
  }
  for (const id of byId.keys()) visit(id);
}

function legacyCases() {
  const manifest = JSON.parse(fs.readFileSync(path.join(ROOT, "manifest.json"), "utf8"));
  return manifest.cases.map((c) => {
    const source = fs.readFileSync(path.join(ROOT, c.file), "utf8");
    return { id: `legacy-${String(c.id).padStart(3, "0")}`, alias: String(c.id).padStart(3, "0"),
      file: c.file, source, hash: hash(source), legacy: true, variants: ["original"], requires: [],
      question: c.operation, axes: ["original fixed input"], memory: false,
      observations: ["whole original workload"], explanations: ["Mixed legacy workload; run isolated contrasts"],
      limitation: "Original fixed input: no reserved seed/source generalization or isolated attribution." };
  });
}

export function loadCatalog() {
  const config = JSON.parse(fs.readFileSync(path.join(ROOT, "experiments.json"), "utf8"));
  if (new Set(config.files).size !== config.files.length) throw new Error("duplicate case file");
  const cases = config.files.map(readCase);
  validateGraph(cases);
  const legacy = legacyCases();
  if (new Set(legacy.map((c) => c.id)).size !== legacy.length) throw new Error("duplicate legacy ID");
  return { config, cases, legacy };
}

export function scenarios(cases, config, qualification = false, size = "small") {
  return cases.flatMap((c) => {
    const sizes = c.legacy ? [["original", 1]] : qualification ? Object.entries(config.sizes) : [[size, config.sizes[size]]];
    const seeds = c.legacy ? [0] : config.seeds[qualification ? "qualification" : "development"];
    return c.variants.flatMap((variant) => sizes.flatMap(([label, n]) => seeds.map((seed) => ({
      id: `${c.id}/${variant}/${label}/${seed}`, experiment: c.id, variant, size: label, n, seed,
      sourceForm: qualification && !c.legacy ? "wrapped" : "direct", legacy: c.legacy
    }))));
  });
}

export function editionIdentity(catalog) {
  const support = ["experiments.json", "manifest.json", "payload.js", "run.mjs",
    ...fs.readdirSync(path.join(ROOT, "lib")).filter((f) => f.endsWith(".mjs")).map((f) => `lib/${f}`)];
  const files = [...new Set([...support, ...catalog.cases.map((c) => c.file), ...catalog.legacy.map((c) => c.file)])].sort();
  const hashes = Object.fromEntries(files.map((f) => [f, hash(fs.readFileSync(path.join(ROOT, f)))]));
  return { schema: 1, edition: catalog.config.edition, digest: hash(JSON.stringify(hashes)), files: hashes };
}
