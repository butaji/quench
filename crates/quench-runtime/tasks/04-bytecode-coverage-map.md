# 04 — V8v7 source-to-stencil coverage map

Status: in_progress

Maintain a line-level report for every V8v7 JavaScript line that cannot run entirely in stencil mode. Each entry must state the unsupported construct, the current fallback or failure, and general-purpose implementation options. Coverage labels must describe present behavior, never aspirational features.

Acceptance: the complete benchmark has no unexplained interpreter execution and no benchmark-specific opcode or fused pattern.
