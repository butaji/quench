#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const tasksDir = path.join(root, 'tasks');
const docsDir = path.join(root, 'docs');
const profilesDir = path.join(root, 'crates', 'quench-runtime', 'testdata', 'execution_profiles');
const indexPath = path.join(tasksDir, 'index.json');
const index = JSON.parse(fs.readFileSync(indexPath, 'utf8'));
const errors = [];
const items = new Map();
const requiredTaskSections = [
  '## Outcome',
  '## Current gap',
  '## Dependencies',
  '## Scope and implementation sequence',
  '## Verification',
  '## Definition of done',
  '## Performance evidence',
];

for (const item of index.items ?? []) {
  const id = String(item.id).replace(/^0+/, '') || '0';
  if (items.has(id)) errors.push(`duplicate task id ${id}`);
  items.set(id, item);
}

for (const item of items.values()) {
  const id = String(item.id).replace(/^0+/, '') || '0';
  if (!['pending', 'in_progress', 'deferred'].includes(item.status)) {
    errors.push(`${id}: unsupported status ${item.status}`);
  }
  const file = path.join(tasksDir, item.file);
  if (!fs.existsSync(file)) {
    errors.push(`${id}: missing ${path.relative(root, file)}`);
    continue;
  }
  const taskText = fs.readFileSync(file, 'utf8');
  const heading = taskText.split(/\r?\n/, 1)[0];
  const expected = `# ${String(item.id).padStart(3, '0')}: ${item.title}`;
  if (heading !== expected) errors.push(`${id}: heading does not match index title`);
  for (const section of requiredTaskSections) {
    if (!new RegExp(`^${section.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`, 'm').test(taskText)) {
      errors.push(`${id}: missing required section ${section}`);
    }
  }
  for (const dependency of item.depends_on ?? []) {
    const dep = String(dependency).replace(/^0+/, '') || '0';
    if (!items.has(dep)) errors.push(`${id}: unknown dependency ${dependency}`);
  }
}

const laneMembers = new Map();
for (const [lane, ids] of Object.entries(index.lanes ?? {})) {
  for (const rawId of ids) {
    const id = String(rawId).replace(/^0+/, '') || '0';
    if (!items.has(id)) errors.push(`lane ${lane}: unknown task ${rawId}`);
    const previous = laneMembers.get(id);
    if (previous) errors.push(`task ${rawId}: appears in lanes ${previous} and ${lane}`);
    laneMembers.set(id, lane);
  }
}

const next = String(index.next_task ?? '').replace(/^0+/, '') || '0';
const critical = (index.lanes?.critical_path ?? []).map(String).map(id => id.replace(/^0+/, '') || '0');
if (!items.has(next)) errors.push(`next_task ${index.next_task} is not an item`);
if (!critical.includes(next)) errors.push(`next_task ${index.next_task} is not on critical_path`);
const active = [...items.entries()].filter(([, item]) => item.status === 'in_progress').map(([id]) => id);
if (active.length > 1) errors.push(`multiple in_progress tasks: ${active.join(', ')}`);
if (active.length === 1 && active[0] !== next) {
  errors.push(`in_progress task ${active[0]} is not next_task ${next}`);
}

const criticalPosition = new Map(critical.map((id, position) => [id, position]));
for (const [position, id] of critical.entries()) {
  const item = items.get(id);
  if (!item) continue;
  for (const rawDependency of item.depends_on ?? []) {
    const dependency = String(rawDependency).replace(/^0+/, '') || '0';
    const dependencyPosition = criticalPosition.get(dependency);
    if (dependencyPosition !== undefined && dependencyPosition >= position) {
      errors.push(`${id}: critical dependency ${rawDependency} is not earlier in critical_path`);
    }
    const lane = laneMembers.get(dependency);
    if (lane === 'profiled' || lane === 'host' || lane === 'deferred') {
      errors.push(`${id}: critical_path cannot depend on ${lane} task ${rawDependency}`);
    }
  }
}

const firstOpen = critical.find(id => items.get(id)?.status !== 'complete');
if (firstOpen && firstOpen !== next) {
  errors.push(`next_task ${index.next_task} is not the first open critical task ${firstOpen}`);
}

for (const [id, item] of items.entries()) {
  const lane = laneMembers.get(id);
  if (!lane) errors.push(`task ${id}: missing lane membership`);
  if (item.status === 'deferred' && lane !== 'deferred') {
    errors.push(`task ${id}: deferred status requires deferred lane`);
  }
  if (lane === 'profiled' || lane === 'host') {
    for (const rawDependency of item.depends_on ?? []) {
      const dependency = String(rawDependency).replace(/^0+/, '') || '0';
      if (criticalPosition.has(dependency)) {
        errors.push(`${id}: ${lane} task depends on critical task ${rawDependency}; make the lane dependency explicit`);
      }
    }
  }
}

// The execution-profile contract is intentionally one corpus, not a second
// expectation set per architecture mode. Keep the machine-checked inventory
// aligned with the count and schema named by the authority pages.
if (fs.existsSync(profilesDir)) {
  const profileFiles = fs.readdirSync(profilesDir)
    .filter(name => name.endsWith('.json'))
    .sort();
  if (profileFiles.length !== 342) {
    errors.push(`execution-profile corpus has ${profileFiles.length} JSON files; expected 342`);
  }
  for (const file of profileFiles) {
    const source = path.join(profilesDir, file);
    let profile;
    try {
      profile = JSON.parse(fs.readFileSync(source, 'utf8'));
    } catch (error) {
      errors.push(`execution-profile ${file}: invalid JSON (${error.message})`);
      continue;
    }
    for (const key of ['schema', 'contract', 'warmup', 'result', 'ir']) {
      if (!(key in profile)) errors.push(`execution-profile ${file}: missing ${key}`);
    }
    if (profile.schema !== 3) errors.push(`execution-profile ${file}: schema is ${profile.schema}, expected 3`);
    if (profile.contract !== 'optimized') errors.push(`execution-profile ${file}: contract is ${JSON.stringify(profile.contract)}, expected "optimized"`);
    if (!Array.isArray(profile.ir)) errors.push(`execution-profile ${file}: ir must be an array`);
  }
} else {
  errors.push('missing execution-profile corpus directory');
}

for (const directory of [docsDir, tasksDir]) {
  for (const file of fs.readdirSync(directory).filter(name => name.endsWith('.md'))) {
    const source = path.join(directory, file);
    const text = fs.readFileSync(source, 'utf8');
    for (const match of text.matchAll(/\btask\s+(\d{3})\b/gi)) {
      const id = match[1].replace(/^0+/, '') || '0';
      if (!items.has(id)) {
        errors.push(`${path.relative(root, source)}: references retired or unknown task ${match[1]}`);
      }
    }
    for (const match of text.matchAll(/\]\(([^)]+)\)/g)) {
      const target = match[1].split('#', 1)[0];
      if (!target || /^(?:https?:|mailto:)/.test(target)) continue;
      const resolved = path.resolve(path.dirname(source), target);
      if (!fs.existsSync(resolved)) errors.push(`${path.relative(root, source)}: missing link ${target}`);
    }
  }
}

// Keep the three evidence levels and the no-ceiling policy aligned across the
// authority pages. These are deliberately small sentinels rather than a prose
// linter: if one authority is edited to make a stronger claim, the check fails
// before that contradiction reaches the roadmap.
const authorityTexts = {
  readme: fs.readFileSync(path.join(docsDir, 'README.md'), 'utf8'),
  contracts: fs.readFileSync(path.join(docsDir, 'execution-contract-tests.md'), 'utf8'),
  performance: fs.readFileSync(path.join(docsDir, 'performance-lanes.md'), 'utf8'),
  architecture: fs.readFileSync(path.join(docsDir, 'architecture.md'), 'utf8'),
  spec: fs.readFileSync(path.join(docsDir, 'stencil-jit-implementation-spec.md'), 'utf8'),
  queue: fs.readFileSync(path.join(tasksDir, 'README.md'), 'utf8'),
  };
const authorityRequirements = [
  ['docs/README.md', '342 execution-profile JSON files describe the'],
  ['docs/README.md', 'A green run proves'],
  ['docs/README.md', 'A generated opcode or selected artifact is not evidence of entry'],
  ['docs/README.md', 'A score never rewrites the IR contract'],
  ['docs/execution-contract-tests.md', 'best *verified target for that policy*'],
  ['docs/execution-contract-tests.md', 'Op::physical_opcode'],
  ['docs/execution-contract-tests.md', 'not a proof that a different lowering or dataflow cannot be better'],
  ['docs/performance-lanes.md', 'V8-v7 is the primary progress indicator'],
  ['docs/performance-lanes.md', 'The default Apple-arm policy is likewise diagnostic'],
  ['docs/architecture.md', 'Call`/`CallSlow` and `Loop`/`ForI`'],
  ['docs/architecture.md', 'Op::LOWERING_MATRIX'],
  ['docs/architecture.md', 'Op::physical_opcode'],
  ['docs/architecture.md', 'stale representatives cannot seed'],
  ['docs/stencil-jit-implementation-spec.md', 'valid edge to another canonical PC outside'],
  ['docs/stencil-jit-implementation-spec.md', 'Op::LOWERING_MATRIX'],
  ['docs/stencil-jit-implementation-spec.md', 'Op::physical_opcode'],
  ['tasks/README.md', 'not a globally optimal IR or machine-code claim'],
  ['tasks/README.md', 'the VM has no arbitrary optimization ceiling'],
  ['docs/architecture.md', 'which owns CFG shape'],
  ['docs/architecture.md', 'OperationSpec::generic_bridge_safe'],
  ['docs/architecture.md', 'Family overrides are declaration markers'],
  ['docs/architecture.md', 'OperationSpec::generic_bridge_safe'],
  ['docs/architecture.md', 'completed cross-tier authority owns helper/re-entry unification'],
  ['docs/architecture.md', 'owner-side `has_current_layout` guard'],
  ['docs/architecture.md', 'installation also rejects superseded representatives'],
  ['docs/architecture.md', 'before acquiring dense backing views'],
  ['docs/architecture.md', 'before mutable backing acquisition'],
];
for (const [file, phrase] of authorityRequirements) {
  const key = file === 'tasks/README.md'
    ? 'queue'
    : ({
      'docs/README.md': 'readme',
      'docs/execution-contract-tests.md': 'contracts',
      'docs/performance-lanes.md': 'performance',
      'docs/architecture.md': 'architecture',
      'docs/stencil-jit-implementation-spec.md': 'spec',
    }[file] ?? '');
  const normalized = authorityTexts[key]?.replace(/\s+/g, ' ');
  if (!normalized?.includes(phrase)) {
    errors.push(`${file}: missing shared evidence invariant ${JSON.stringify(phrase)}`);
  }
}

if (errors.length) {
  for (const error of errors) console.error(`error: ${error}`);
  process.exit(1);
}

console.log(`task/documentation coherence: ok (${items.size} tasks, ${fs.readdirSync(docsDir).filter(name => name.endsWith('.md')).length} docs)`);
