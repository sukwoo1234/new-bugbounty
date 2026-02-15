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

## Git Conventions
- Branches: `main`, `feat/<topic>`, `fix/<topic>`, `docs/<topic>`
- After a work unit, propose commit/push flow and request approval before running git commands.

## Security/Policy Defaults
- Follow HackerOne general policy for scope and disclosure.
- Target downloads must come from official release channels with pinned versions and recorded hashes.
