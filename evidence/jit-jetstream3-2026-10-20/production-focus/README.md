# Production JetStream JIT focus screen

This is a one-round, source-clean production-profile screen on Linux x86_64
(4 vCPUs, 16 GiB container memory limit), using the pinned JetStream 3.0
corpus at revision `06785cf`. It measures `web-ssr` and `hash-map` against
Quench JIT, Bun 1.4.2 JIT, and Node 26.10.0 JIT. Both fixtures validated.

| Fixture | Quench score | Bun score | Node score | Quench peak RSS | Bun peak RSS | Node peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| web-ssr | 11.38 | 83.29 | 96.33 | 130.5 MiB | 239.9 MiB | 200.0 MiB |
| hash-map | 4.22 | 250.23 | 165.19 | 74.0 MiB | 96.9 MiB | 161.5 MiB |

This is diagnostic evidence only. One round does not qualify a comparison.
Quench remains behind both peers on score in both fixtures; its peak RSS is
lower in these samples. Do not infer a stable score or RSS advantage without
matched rounds.

The full-suite one-round attempt was stopped during `babylon-wtb` while the
iteration-profile Quench process was still running. It produced no report and
is not part of this evidence. The recorded report here was produced after a
clean production build from source revision `8b5904f2e4b129b1cc56e2843cb87d8b4e6aa15e`.
