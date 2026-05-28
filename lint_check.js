const fs = require('fs');
const path = require('path');

const files = [
  'src/transpile/analyzer.rs',
  'src/transpile/hir.rs',
  'src/runtime/signals.rs',
  'src/runtime/middleware.rs',
  'src/runtime/islands.rs',
  'src/runtime/vdom.rs',
  'src/commands/ssr.rs',
  'src/commands/parallel.rs',
  'src/transpile/jsx_transformer.rs',
  'src/transpile/middlewaregen.rs',
  'src/transpile/routegen.rs',
  'src/transpile/errors.rs',
  'src/commands/incremental.rs',
  'src/commands/routes.rs',
  'src/runtime/server.rs',
];

const MAX_FILE_LINES = 500;
const MAX_FN_LINES = 40;
const MAX_FN_COMPLEXITY = 10;

function detect_fn_name(line) {
  const code = line.split('//')[0].trim();
  if (!code || code.endsWith(';')) return null;
  const fn_idx = code.indexOf('fn ');
  if (fn_idx < 0) return null;
  if (fn_idx > 0) {
    const prev = code.charCodeAt(fn_idx - 1);
    if ((prev >= 48 && prev <= 57) || (prev >= 65 && prev <= 90) || (prev >= 97 && prev <= 122) || prev === 95 || prev === 58) {
      return null;
    }
  }
  const after = code.slice(fn_idx + 3);
  const name = after.trim().split(/[^a-zA-Z0-9_<>]/)[0].replace(/<$/, '');
  if (!name) return null;
  const after_name = after.slice(name.length);
  if (!after_name.includes('(') && !after_name.includes('<')) return null;
  return name;
}

function find_matching_brace(lines, start) {
  let depth = 0;
  let in_str = false;
  let delim = '\0';
  let esc = false;
  for (let idx = start - 1; idx < lines.length; idx++) {
    const code = lines[idx].split('//')[0] || '';
    for (let i = 0; i < code.length; i++) {
      const ch = code[i];
      if (in_str) {
        if (esc) { esc = false; continue; }
        if (ch === '\\') { esc = true; continue; }
        if (ch === delim) { in_str = false; }
        continue;
      }
      if (ch === '"' || ch === "'") {
        in_str = true; delim = ch; continue;
      }
      if (ch === '{') depth++;
      else if (ch === '}') {
        depth--;
        if (depth === 0) return idx + 1;
      }
    }
  }
  return null;
}

function find_fn_body(lines, fn_line_idx) {
  for (let offset = 0; offset < 5; offset++) {
    const idx = fn_line_idx + offset;
    if (idx >= lines.length) break;
    const code = lines[idx].split('//')[0].trim();
    if (offset === 0) {
      const pos = code.indexOf('{');
      if (pos >= 0 && code.slice(0, pos).includes(')')) {
        return [idx + 2, find_matching_brace(lines, idx + 1)];
      }
    } else {
      if (code.startsWith('{')) return [idx + 2, find_matching_brace(lines, idx + 1)];
      if (code.includes('{') && !code.includes('fn ')) return [idx + 2, find_matching_brace(lines, idx + 1)];
    }
  }
  return null;
}

function compute_complexity(lines_slice) {
  let c = 1;
  for (const line of lines_slice) {
    const line_str = line.split('//')[0];
    c += (line_str.match(/if /g) || []).length;
    c += (line_str.match(/else if /g) || []).length;
    c += (line_str.match(/while /g) || []).length;
    c += (line_str.match(/for /g) || []).length;
    c += (line_str.match(/loop {/g) || []).length;
    c += (line_str.match(/match /g) || []).length;
    c += (line_str.match(/ => /g) || []).length;
    c += (line_str.match(/ && /g) || []).length;
    c += (line_str.match(/ \|\| /g) || []).length;
    c += (line_str.match(/\?/g) || []).length;
  }
  return c;
}

for (const f of files) {
  const content = fs.readFileSync(f, 'utf8');
  const lines = content.split('\n');
  const code_lines = lines.filter(l => {
    const t = l.trim();
    return t && !t.startsWith('//') && !t.startsWith('/*');
  }).length;

  const violations = [];
  if (code_lines > MAX_FILE_LINES) {
    violations.push(`[FILE_TOO_LONG] ${f}: ${code_lines} code lines (max ${MAX_FILE_LINES})`);
  }

  for (let i = 0; i < lines.length; i++) {
    const name = detect_fn_name(lines[i]);
    if (name) {
      const body = find_fn_body(lines, i);
      if (body && body[1]) {
        const line_count = body[1] - (i + 1) + 1;
        const complexity = compute_complexity(lines.slice(i, body[1]));
        if (line_count > MAX_FN_LINES) {
          violations.push(`[FN_TOO_LONG] ${f}::${name}: ${line_count} lines (max ${MAX_FN_LINES}) at line ${i+1}`);
        }
        if (complexity > MAX_FN_COMPLEXITY) {
          violations.push(`[FN_TOO_COMPLEX] ${f}::${name}: complexity ${complexity} (max ${MAX_FN_COMPLEXITY}) at line ${i+1}`);
        }
      }
    }
  }

  if (violations.length > 0) {
    console.log('\n' + violations.join('\n'));
  }
}
