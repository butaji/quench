import random, sys
random.seed(int(sys.argv[1]))
V=["a","b","c","x"]
def atom(d):
    r=random.random()
    if d<=0 or r<0.3: return random.choice(V+["1","2","3"])
    if r<0.45: return f"({random.choice(V)} = {expr(d-1)})"
    if r<0.5: return f"({random.choice(V)}{random.choice(['++','--'])})"
    if r<0.55: return f"({random.choice(['++','--'])}{random.choice(V)})"
    if r<0.62: return f"({random.choice(V)} {random.choice(['+=','-=','|=','^='])} {expr(d-1)})"
    if r<0.7: return f"({expr(d-1)}, {expr(d-1)})"
    if r<0.78: return f"({expr(d-1)} ? {expr(d-1)} : {expr(d-1)})"
    if r<0.85: return f"({expr(d-1)} {random.choice(['&&','||','??'])} {expr(d-1)})"
    if r<0.9: return f"g({expr(d-1)}, {expr(d-1)})"
    return f"[{expr(d-1)}, {expr(d-1)}][{random.choice(['0','1'])}]"
def expr(d):
    if random.random()<0.5: return atom(d)
    return f"({atom(d)} {random.choice(['+','-','^','|','&','*','<<','>>>','<','==='])} {atom(d)})"
def stmt(d):
    r=random.random()
    if r<0.3: return f"{random.choice(V)} = {expr(d)};"
    if r<0.5: return ", ".join(f"{random.choice(V)} = {expr(d)}" if random.random()<0.7 else expr(d) for _ in range(random.randint(2,5)))+";"
    if r<0.65: return f"{expr(d)} ? {random.choice(V)} = {expr(d)} : ({random.choice(V)} = {expr(d)}, {random.choice(V)}++);"
    if r<0.8: return f"{expr(d)} && ({random.choice(V)} = {expr(d)}, {random.choice(V)} = {expr(d)});"
    if r<0.9: return f"out.push({expr(d)});"
    return f"for (let i = 0; i < 3; i++) {{ {stmt(d-1)} }}"
fns=[]
for k in range(60):
    body=" ".join(stmt(3) for _ in range(random.randint(3,7)))
    fns.append(f"function f{k}(a, b) {{ var c = 3, x = 4; {body} out.push(a, b, c, x); }}")
print("const out = []; let calls = 0; function g(p, q) { calls++; return (p | 0) + (q | 0) * 2; }")
print("\n".join(fns))
print("const F = [" + ", ".join(f"f{k}" for k in range(60)) + "]; for (let k = 0; k < 60; k++) { try { F[k](1, 2); F[k](7, -3); } catch (e) { out.push(\"E\" + k + e.constructor.name); } }")
print("console.log(JSON.stringify(out.map(v => typeof v === 'number' && !Number.isFinite(v) ? String(v) : v)), calls);")
