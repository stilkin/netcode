# Tasks

## 1. build-pipeline (Local commit gates)
- [x] 1.1 Delete `.github/workflows/ci.yml` (and the `.github/` tree).
- [x] 1.2 Verify `.githooks/pre-commit` runs fmt + clippy + tests and rejects failing commits (already active via `core.hooksPath`).

## 2. Documentation truthfulness
- [x] 2.1 README: replace the CI row in Technologies with the local hook; reword Contributing comment.
- [x] 2.2 docs/development.md: drop the workflow line from the layout; reword Checks section; note the Windows caveat.
- [x] 2.3 docs/architecture.md: reword the pure-Rust rationale (no CI matrix).

## 3. Wrap-up
- [x] 3.1 Full local gate run green; `openspec validate replace-ci-with-local-gates --strict` green.
- [x] 3.2 Mark roadmap entries, `openspec archive replace-ci-with-local-gates --yes`, commit, push.
