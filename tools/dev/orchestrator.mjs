#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

const root = process.cwd();
const dir = path.join(root, '.orchestrator');
const statePath = path.join(dir, 'state.json');
const lockPath = path.join(dir, 'lock');
const indexPath = path.join(root, 'tasks', 'index.json');
const DEFAULT_LEASE = 30 * 60 * 1000;

function out(value, code = 0) { process.stdout.write(JSON.stringify(value) + '\n'); process.exitCode = code; }
function die(message, code = 1) { out({ ok: false, error: message }, code); }
function args() {
  const a = process.argv.slice(2);
  if (a[0] === '--help' || a[0] === '-h') return { command: 'help', opts: { help: true } };
  const command = a.shift() || 'status', opts = {};
  for (let i = 0; i < a.length; i++) {
    const x = a[i];
    if (x === '--help' || x === '-h') opts.help = true;
    else if (x.startsWith('--')) { const k = x.slice(2).replaceAll('-', '_'); opts[k] = a[i + 1]?.startsWith('--') ? true : (a[i + 1] ?? true); if (opts[k] !== true) i++; }
    else if (!opts.id) opts.id = x;
  }
  return { command, opts };
}
function taskId(v) { const n = String(v ?? '').replace(/^0+/, '') || '0'; return String(Number(n)); }
function displayId(id) { return String(id).padStart(3, '0'); }
function readIndex() {
  const data = JSON.parse(fs.readFileSync(indexPath, 'utf8'));
  const tasks = new Map(data.items.map(t => [taskId(t.id), { ...t, id: taskId(t.id), depends_on: (t.depends_on || []).map(taskId) }]));
  return tasks;
}
function readState() { return JSON.parse(fs.readFileSync(statePath, 'utf8')); }
function atomic(file, value) {
  const tmp = `${file}.${process.pid}.${Date.now()}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(value, null, 2) + '\n', { mode: 0o600 });
  fs.renameSync(tmp, file);
}
function lock() {
  fs.mkdirSync(dir, { recursive: true });
  try { fs.mkdirSync(lockPath); fs.writeFileSync(path.join(lockPath, 'owner'), JSON.stringify({ pid: process.pid, at: Date.now() })); }
  catch { die('orchestrator lock is held; retry later', 2); throw new Error('locked'); }
  return () => { try { fs.rmSync(lockPath, { recursive: true, force: true }); } catch {} };
}
function save(s) { s.updated_at = new Date().toISOString(); atomic(statePath, s); }
function init() {
  const tasks = readIndex(); const now = new Date().toISOString();
  const state = { version: 1, created_at: now, updated_at: now, lease_ms: DEFAULT_LEASE, tasks: Object.fromEntries([...tasks].map(([id, t]) => [id, { id, status: 'pending', attempts: 0, depends_on: t.depends_on, title: t.title }])) };
  atomic(statePath, state); return { ok: true, initialized: tasks.size, state: statePath };
}
function ensure() { if (!fs.existsSync(statePath)) throw new Error('state is not initialized; run init'); return { tasks: readIndex(), state: readState() }; }
function stale(s, t) { return t.status === 'in_progress' && Date.now() - Date.parse(t.claimed_at || 0) > (s.lease_ms || DEFAULT_LEASE_MS); }
function recoverState(s) { let count = 0; for (const t of Object.values(s.tasks)) if (stale(s, t)) { t.status = 'pending'; t.recovered_at = new Date().toISOString(); t.recovery_reason = 'stale lease'; delete t.claimed_at; delete t.claimed_by; count++; } return count; }
function main() {
  const { command, opts } = args();
  if (opts.help) return out({ ok: true, usage: 'orchestrator.mjs <status|init|claim|complete|fail|recover|reset> [id] [options]', options: { '--evidence-command CMD': 'run command and capture successful evidence', '--evidence-result TEXT': 'explicit evidence supplied by caller', '--lease-ms N': 'lease duration for claim' } });
  if (command === 'init') { const release = lock(); try { return out(init()); } finally { release(); } }
  let release; try { release = lock(); const { tasks, state } = ensure();
    if (command === 'status') { const recovered = recoverState(state); if (recovered) save(state); return out({ ok: true, recovered, tasks: state.tasks }); }
    if (command === 'recover') { const recovered = recoverState(state); save(state); return out({ ok: true, recovered }); }
    if (command === 'reset') { if (opts.id) { const id = taskId(opts.id); if (!state.tasks[id]) throw new Error(`unknown task ${displayId(id)}`); state.tasks[id] = { ...state.tasks[id], status: 'pending', attempts: state.tasks[id].attempts || 0 }; delete state.tasks[id].claimed_at; delete state.tasks[id].claimed_by; } else for (const t of Object.values(state.tasks)) { t.status = 'pending'; delete t.claimed_at; delete t.claimed_by; } save(state); return out({ ok: true, reset: opts.id ? [displayId(taskId(opts.id))] : 'all' }); }
    const id = taskId(opts.id); const t = state.tasks[id]; const meta = tasks.get(id); if (!t || !meta) throw new Error(`unknown task ${displayId(id)}`);
    if (command === 'claim') { recoverState(state); const blocked = meta.depends_on.filter(d => state.tasks[d]?.status !== 'done'); if (blocked.length) throw new Error(`dependencies incomplete: ${blocked.map(displayId).join(', ')}`); if (t.status !== 'pending') throw new Error(`task is ${t.status}`); t.status = 'in_progress'; t.attempts = (t.attempts || 0) + 1; t.claimed_at = new Date().toISOString(); t.claimed_by = opts.worker || `pid:${process.pid}`; save(state); return out({ ok: true, task: t, executor: meta.executor || null }); }
    if (command === 'complete') { if (t.status !== 'in_progress') throw new Error('task must be in_progress'); let evidence = opts.evidence_result; if (opts.evidence_command) { try { evidence = execFileSync('/bin/sh', ['-lc', String(opts.evidence_command)], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }); } catch (e) { throw new Error(`evidence command failed: ${e.stderr?.trim() || e.message}`); } } if (!evidence || !String(evidence).trim()) throw new Error('explicit --evidence-command or --evidence-result is required'); t.status = 'done'; t.completed_at = new Date().toISOString(); t.evidence = { result: String(evidence) }; delete t.claimed_at; delete t.claimed_by; save(state); return out({ ok: true, task: t }); }
    if (command === 'fail') { if (t.status !== 'in_progress') throw new Error('task must be in_progress'); t.status = 'failed'; t.failed_at = new Date().toISOString(); t.error = opts.reason || opts.message || 'failed'; delete t.claimed_at; delete t.claimed_by; save(state); return out({ ok: true, task: t }); }
    throw new Error(`unknown command: ${command}`);
  } catch (e) { if (e.message !== 'locked') die(e.message); } finally { release?.(); }
}
main();
