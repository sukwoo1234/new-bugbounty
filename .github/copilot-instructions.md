# Copilot instructions (bugbounty)

## Project context
- This repo is currently **documentation-first**: the source of truth is the roadmap in [first.md](../first.md).
- Work is guided by the to-do list in [docs/todo.md](../docs/todo.md) and collaboration rules in [docs/rules.md](../docs/rules.md).
- No codebase exists yet; do not invent implementation details beyond documented decisions.

## Product goals (from first.md)
- Build a bug bounty fuzzing platform that **reproduces + validates + reports** crashes.
- Primary targets: **GGUF, ONNX, safetensors**.
- Valid bug definition: **SEGV/Abort + same input 3x + top 3 stack frames match**.
- CLI-first UX; dashboard later.

## Required policies & constraints
- **HackerOne general policy** only; out-of-scope tests are forbidden.
- Use official release sources; **version is pinned and recorded**; **file hash stored**.
- Storage policy: keep artifacts **30 days**, **core dump OFF by default**, logs **zstd**; keep only reproducible crashes.

## Execution/flows to preserve
- CLI commands (planned): `tool run`, `tool triage`, `tool report`.
- Results UX: `list`, `show <id>`, `export <id>`.
- Default paths: data in `./data`, seeds in `./seeds`.
- Repro timeouts: 60s execution, 30s hang; retry 3x; flaky = 1/3 repro; retry once then discard.

## What to update when editing docs
- If you change project decisions, update [first.md](../first.md) **and** the checklist in [docs/todo.md](../docs/todo.md).
- Follow collaboration rules in [docs/rules.md](../docs/rules.md): ask before new info, summarize before edits, and request approval before commands.

## Gaps intentionally left as TBD
- Harness **API/function names** are TBD (to be filled during implementation).
- Report template **sample** and metrics definitions are still open items.
