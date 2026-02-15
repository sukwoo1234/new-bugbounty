# Project Agent Rules

## Workflow
- Ask before executing non-read commands.
- Summarize planned file changes before editing.
- If scope grows, split into smaller agreed tasks.
- At each meaningful milestone, ask: `to-do 리스트 갱신할까요?`
- At major decision points or session end, ask: `새 대화용 컨텍스트 요약 만들어줘?`

## Boundaries
- Do not run installs or heavy automation unless requested.
- Keep proposals outside requested scope as optional.
- Prefer the simplest solution that satisfies the request.
- Avoid speculative abstractions or unrequested features.
- Limit edits to lines directly related to the current request.

## Execution Quality
- State assumptions explicitly before implementation when ambiguity exists.
- If requirements are unclear or conflicting, ask before coding.
- Define verifiable success criteria for each task and validate before closing.
- If a simpler approach exists, present it before implementing a complex one.
- If your change creates unused code/imports, clean up only what your change introduced.

## Git Conventions
- Branches: `main`, `feat/<topic>`, `fix/<topic>`, `docs/<topic>`
- After a work unit, propose commit/push flow and request approval before running git commands.

## Security/Policy Defaults
- Follow HackerOne general policy for scope and disclosure.
- Target downloads must come from official release channels with pinned versions and recorded hashes.
