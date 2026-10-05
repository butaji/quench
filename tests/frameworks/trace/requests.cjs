const { registerHooks, builtinModules } = require('node:module');
const seen = new Map();
const isB = s => s.startsWith('node:') || builtinModules.includes(s.split('/')[0]) && builtinModules.includes(s);
registerHooks({ resolve(spec, ctx, next) {
  if (isB(spec) && ctx.parentURL && ctx.parentURL.includes('node_modules')) {
    const pkg = ctx.parentURL.split('node_modules/').pop().split('/').slice(0, ctx.parentURL.split('node_modules/').pop().startsWith('@') ? 2 : 1).join('/');
    const b = spec.replace(/^node:/, ''); if (!seen.has(b)) seen.set(b, new Set()); seen.get(b).add(pkg);
  }
  return next(spec, ctx);
}});
process.on("exit", () => { require("fs").writeFileSync(process.env.OUT, JSON.stringify(Object.fromEntries([...seen].sort().map(([b,p])=>[b,[...p].sort()])))); });
