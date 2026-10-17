# SDD ledger - plan: docs/superpowers/plans/2026-10-17-codebase-organization.md

## Baseline

- Worktree: `C:\Users\12182\.codex\worktrees\codebase-organization\WUHUICL`
- Base commit: `a59fa62` (planning docs)
- Original code baseline: `fc6a698`
- Baseline checks: `npm run typecheck` pass; `npm run build:web` pass; `cargo test --lib`
  158 pass, 2 ignored; `cargo fmt --all -- --check` fail; `npm run format:check` fail in seven
  VoxLink files.

## Parallel Dispatch

| Package | Agent | Write scope |
| --- | --- | --- |
| A - application shell | Erdos `01a149dd-287e-7290-a836-51a00380f69a` | `src/app`, `src/core`, shell/core facades, `App.vue`, `main.ts`, `api.ts`, `store.ts` |
| B - feature conventions | Einstein `01a149dd-2c09-74b2-af6a-4d0c465ed289` | `src/features`, `src/components`, `src/views` |
| C - Rust layout | Plato `01a149dd-2f29-7fa1-a822-33558dcb83ab` | `src-tauri/src` |
| D - quality guardrails | Leibniz `01a149dd-33eb-7883-b64a-75de740a3e51` | configs, scripts, tests, CI, quality docs |

## Preflight Conflict Scan

| Pair | Shared interface/file | Finding | Ruling |
| --- | --- | --- | --- |
| A x B | `src/api.ts` / feature imports | B is told to keep legacy `api` imports; A creates facades. | Coordinator switches feature imports after both reports. |
| A x B | `src/components` / feature modals | B owns feature components; A may only edit `App.vue`. | A keeps modal components in feature paths; B preserves old-path shims. |
| B x D | `src/components/views` formatting | D must not edit app source; B owns moves and formatting. | Coordinator runs final formatting. |
| C x D | Cargo formatting/test | C owns Rust source; D owns workflow only. | No overlap. |
| A x D | `package.json` / lint config | A does not edit package metadata; D does. | No overlap. |
| A x B | `src/features` | A may create only needed facades; B owns feature packages. | Coordinator resolves any file collision. |
| B x C | IPC contract | Both must preserve channel names, payloads, errors. | Explicit final contract audit. |
| A/C/D x docs | Plan/docs | Docs are coordinator-owned and gitignored but tracked. | Workers must not edit docs. |

## Progress

- Planning/spec: complete (`a59fa62`).
- Implementation: 4 parallel workers dispatched.
- Package A: implemented `6c02539..385aa04`; App.vue is 12 lines, shell/core boundaries
  present, typecheck/build pass. Review note: shell implementation is split into ~1.2k-line
  files; core/ipc facade is the preferred bridge for feature code.
- Package B: clean implementation source `a59fa62..1bee399` on
  `codex/features-work-package-b`; typecheck/build pass, Settings 703 lines, Community 593
  lines, 12 feature packages. Fix `1bee399` centralizes the last bridge access.
- Package C: implemented at `746e21c`; Rust app/ipc composition layers and feature modules
  added, 158 tests pass. Review in progress.
- Package D: initial implementation `1d0893d`; lint/tests/checker/CI added. Review requested
  tightening because Prettier ignored all `src/` and `shared/` and correctness lint rules
  were globally disabled.
- Package A fix: `820b216` moves appearance watcher installation into shell lifecycle.
- Package B fix: `1bee399` centralizes feature bridge access and removes empty scaffolds.
- Package C follow-up: `bf9ce7c` moves mods/installer into `features/library` and splits
  archive/fingerprint/scan/planning/version modules; `5d71a92` formats the Rust tree.
- Package D follow-up: `6563526` narrows formatting/lint exemptions to explicit files.
- Integration follow-up: implementation views are moved into their owning feature packages,
  the old `src/views/*.vue` files remain compatibility shims, direct feature access to
  `window.kamucl` is routed through `src/core/ipc`, and cross-feature imports use public
  barrels.
- Rust integration fix: `features/library/mods.rs` imports the split planning helper from its
  actual module, and composed IPC handlers are cast to the shared `Handler` type.
- Verification before integration commit: TypeScript pass; Vite build pass (456 modules);
  Node unit tests 13/13 pass; architecture check pass (231 source files, 86 explicit legacy
  allowlist entries, 0 violations); ESLint completes with 0 errors and 106 legacy warnings;
  Prettier pass; `cargo fmt --all -- --check` pass; `cargo check --lib` pass.
- Rust test runtime: the test binary links successfully but exits before the harness with
  Windows `STATUS_ENTRYPOINT_NOT_FOUND` (`0xc0000139`). The imported ApiSet
  `api-ms-win-core-synch-l1-2-0.dll` resolves to an older System32 forwarder while the
  installed Windows SDK contains the newer downlevel copy. A clean D-drive target rebuild
  reproduces the host failure, so it is not a stale project artifact. No system files were
  modified.

## Rulings

- Ruling: integrate from the clean Package B branch (`a59fa62..1bee399`) instead of the
  shared mixed history (`42b624e`, `385aa04`). Why: the shared index produced commits whose
  trees include other packages, so parent-range diffs are not reviewable. Cost if wrong:
  coordinator must semantically merge A/C/D files rather than replay clean commits.
- Ruling: the local `src/features/community/data/featureBridge.ts` is temporary; replace it
  with Package A's `src/core/ipc` facade. Why: feature code must not access `window.kamucl`.
  Cost if wrong: a typed adapter must be extended for memory/path/platform helpers.
- Ruling: broad `src/`/`shared/` Prettier ignores and global disabling of parse/correctness
  lint rules are not acceptable guardrails. Why: they make checks pass without checking the
  code. Cost if wrong: some legacy violations need explicit, removal-dated exemptions.
- Ruling: Cargo link/test cannot be completed in the current host because `link.exe` and the
  Windows SDK library tree are absent. Why: `cargo fmt` and source-level checks still run,
  and the original tree's test artifacts prove the pre-refactor baseline. Cost if wrong:
  hidden Rust compile/link errors remain unverified until a machine with MSVC Build Tools
  and Windows SDK runs the CI workflow.
- Ruling: the earlier missing-MSWC blocker is superseded by the completed Visual Studio
  installation. Why: `cargo check --lib` and test-binary linking now run, leaving only the
  host ApiSet runtime failure. Cost if wrong: none for project source; the remaining runtime
  uncertainty must be verified on a host with a consistent Windows ApiSet installation.
