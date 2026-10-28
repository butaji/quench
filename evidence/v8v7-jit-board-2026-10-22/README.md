# V8-v7 JIT-mode Score board, 2026-10-22

One-round screen (not a qualification) of `quench-node --jit` at 389240cbf1 (release build)
against Node v22.22.0 and Bun 1.4.2 with their JITs enabled, and QuickJS 2025-04-26
(no JIT). Stock V8-v7 fixtures with the repository's base.js harness (`mat-*.js`: base.js + fixture + RunSuites).

Host: Intel(R) Xeon(R) Processor @ 2.10GHz, family 6 model 207 (Emerald Rapids), 4 vCPU, Linux 6.18.44-fc-v140.

## Quench --jit vs Node and Bun
```
richards quench=1753 node=47495 bun=42115 vsNode=0.04 vsBun=0.04
deltablue quench=1883 node=94793 bun=18751 vsNode=0.02 vsBun=0.1
crypto quench=6879 node=43985 bun=50972 vsNode=0.16 vsBun=0.13
raytrace quench=3948 node=60383 bun=43955 vsNode=0.07 vsBun=0.09
earley-boyer quench=6215 node=71833 bun=30430 vsNode=0.09 vsBun=0.2
regexp quench=3430 node=8118 bun=11807 vsNode=0.42 vsBun=0.29
splay quench=6276 node=10855 bun=26748 vsNode=0.58 vsBun=0.23
navier-stokes quench=36449 node=43481 bun=35468 vsNode=0.84 vsBun=1.03
```

## Quench --jit vs QuickJS
```
richards qjs=683 quench-jit=1736 ratio=2.54
deltablue qjs=736 quench-jit=1692 ratio=2.3
crypto qjs=1013 quench-jit=6564 ratio=6.48
raytrace qjs=1004 quench-jit=3577 ratio=3.56
earley-boyer qjs=1501 quench-jit=6771 ratio=4.51
regexp qjs=268 quench-jit=3789 ratio=14.14
splay qjs=1693 quench-jit=5654 ratio=3.34
navier-stokes qjs=1727 quench-jit=38162 ratio=22.1
```

Quench --jit leads QuickJS on all eight fixtures and Bun on NavierStokes only. The others need
function-entry compilation with guarded field access and inline-cache calls (jit-design.md,
"Research convergence and next gates"); NavierStokes against Node needs lin_solve below its
serial chain latency (row interleaving), see the TOD-167 notes.
