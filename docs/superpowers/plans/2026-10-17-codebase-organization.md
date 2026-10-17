# WUHUICL Code Organization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax for
> tracking.

**Goal:** Move WUHUICL from a flat legacy layout to a feature-first structure with enforced
dependency boundaries, without changing IPC contracts, persisted data, or user-visible
behavior.

**Architecture:** A strangler migration creates target directories and compatibility
facades, then moves the application shell, feature packages, and Rust subsystems behind
stable public APIs. Application shell code composes features; features depend only on
`core`, `shared`, and their own internals.

**Tech Stack:** Vue 3, TypeScript, Vite, Tauri 2, Rust 2021, Prettier, ESLint, Node test
runner, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-17-codebase-organization-design.md`

## Global Constraints

- Preserve every IPC channel string, event name, argument order, JSON field, error message,
  local-storage key, and persisted file format.
- Preserve existing UI copy and visual behavior.
- Preserve `SPDX-License-Identifier` headers on moved files.
- Do not move icons, binary resources, generated `dist`/`target` output, or vendored Three.js
  model code unless a task explicitly names it.
- Do not edit files outside a task's declared write set.
- Use compatibility re-exports during migration; do not break old import paths in the same
  change that moves a module.
- Keep each worker's writes disjoint. The coordinator integrates and resolves shared files
  such as `package.json`, `vite.config.ts`, `src/main.ts`, and Rust composition entry points.

## Parallel Work Packages

| Package | Write scope | Primary outcome |
| --- | --- | --- |
| A. Application shell | `src/app/**`, `src/core/ipc/**`, `src/core/runtime/**`, `src/core/assets/**`, `src/core/text/**`, `src/shared/ui/**`, shell-only facades | Root shell, IPC, bootstrap, overlays, and UI primitives have target homes |
| B. Feature conventions | `src/features/**`, `src/components/**`, `src/views/**` | Feature packages, public exports, and at least two large views split into focused components |
| C. Rust layout | `src-tauri/src/**` | Rust target modules, at least two feature packages, thin application/IPC composition |
| D. Quality guardrails | `scripts/**`, `.github/workflows/**`, project lint/test config, root docs | Formatting, lint, architecture checks, frontend tests, and CI are reproducible |

Package B consumes `src/core/ipc/index.ts` and shell contracts from Package A. Until that
facade exists, Package B may keep imports to `src/api.ts`; the coordinator will perform the
final facade switch. Package D must not modify application source.

---

### Task 1: Establish Application And Core Boundaries

**Files:**
- Create: `src/app/bootstrap/index.ts`
- Create: `src/app/shell/AppShell.vue`
- Create: `src/app/shell/useAppShell.ts`
- Create: `src/app/overlays/AppOverlays.vue`
- Create: `src/app/state/index.ts`
- Create: `src/core/ipc/index.ts`
- Create: `src/core/ipc/bridge.ts`
- Create: `src/core/runtime/index.ts`
- Create: `src/core/assets/index.ts`
- Create: `src/core/text/index.ts`
- Create: `src/shared/ui/index.ts`
- Modify: `src/App.vue`, `src/main.ts`, `src/api.ts`, `src/store.ts`

**Interfaces:**
- Consumes: existing `window.kamucl`, `src/api.ts`, and `src/store.ts` public functions.
- Produces: `src/core/ipc/index.ts`, which re-exports the existing typed facade and exposes
  `invokeIpc<T>(channel: string, ...args: unknown[]): Promise<T>` and
  `subscribeIpc<T>(channel: string, callback: (payload: T) => void): () => void`.
- Produces: `src/app/state/index.ts`, which re-exports `store`, `ViewName`, and existing
  actions so feature code can migrate without changing behavior.

- [ ] **Step 1: Inventory root shell dependencies**

Run:

```powershell
rg -n "from ['\"](\./|\.\./)+(api|store|App|main|idleTrim|modalFocus|bootTasks)" src -g '*.ts' -g '*.vue'
```

Expected: every application-shell dependency is listed before files move.

- [ ] **Step 2: Create compatibility facades**

Create `src/core/ipc/index.ts` as the sole typed IPC surface. It re-exports the existing
functions from `src/api.ts` and owns future feature API modules. Create
`src/app/state/index.ts` as the shell-state public surface, re-exporting `store`, `ViewName`,
and existing actions from `src/store.ts`.

```ts
// src/core/ipc/index.ts
export * from '../../api';
export { installModalFocus } from '../../modalFocus';
export { startIdleTrim } from '../../idleTrim';
```

The facade must not change the implementation of `invoke`, event subscriptions, or JSON
cleaning. Moving the implementation into `src/core/ipc/bridge.ts` is a second step after all
imports resolve through the facade.

- [ ] **Step 3: Move shell-only modules behind facades**

Move boot/resource helpers to `src/core/runtime`, managed-asset helpers to `src/core/assets`,
Markdown helpers to `src/core/text`, and generic UI primitives to `src/shared/ui`. Leave
one-line re-export shims at each old path until Task 8 switches consumers.

- [ ] **Step 4: Extract the root shell from `App.vue`**

Move navigation rendering, window controls, overlays, and application-level handlers into
`AppShell.vue`, `useAppShell.ts`, and `AppOverlays.vue`. `App.vue` should become a small
composition root that imports the shell and passes explicit props.

Do not move feature-specific data flows into the shell. Each extracted module must own one
of: navigation, window lifecycle, overlays, or global bootstrap.

- [ ] **Step 5: Verify structure and behavior**

Run:

```powershell
npm run typecheck
npm run build:web
npm run format:check
```

Expected: all three commands pass, and `App.vue` is smaller than 1,500 lines.

---

### Task 2: Create Frontend Feature Packages

**Files:**
- Create: `src/features/accounts/{data,ui}/**`
- Create: `src/features/settings/{data,ui}/**`
- Create: `src/features/home/{data,ui}/**`
- Create: `src/features/instances/{data,ui}/**`
- Create: `src/features/mods/{data,ui}/**`
- Create: `src/features/community/{data,ui}/**`
- Create: `src/features/connectivity/{data,ui}/**`
- Create: `src/features/skins/{data,ui,vendor}/**`
- Create: `src/features/launch/{data,ui}/**`
- Create: `src/features/updates/{data,ui}/**`
- Create: `src/features/appearance/{data,ui}/**`
- Create: each feature's `index.ts`
- Modify: feature-owned files under `src/components/**` and `src/views/**`

**Interfaces:**
- Consumes: `src/core/ipc/index.ts`, `src/app/state/index.ts`, and existing shared domain
  helpers.
- Produces: one `index.ts` per feature. Views and components outside a feature import that
  public entry only.
- Produces: feature state modules with stable names such as `useCommunityState`,
  `useSettingsState`, and `useLaunchState`; each returns refs and commands, not raw singleton
  mutation.

- [ ] **Step 1: Map each legacy file to one feature**

Produce a checked-in table in the first feature `README.md` or the feature barrel. Every
moved file must have exactly one owner. Shared files with more than one plausible owner stay
in `shared` until ownership is clear.

- [ ] **Step 2: Move the first two packages completely**

Start with `settings` and `community` because they exercise large views, shared state, modals,
and IPC. Move their views, feature composables, components, API calls, and pure helpers.
Keep old paths as re-export shims.

- [ ] **Step 3: Split the two largest views**

For `SettingsView.vue`, extract settings sections, update controls, and modal coordination
into feature UI components. For `CommunityView.vue`, extract search/filter controls, result
list, details pane, and download queue composition into feature UI components.

Each extracted component receives props and emits commands; it must not import the root
store directly when a small state module can own the state.

- [ ] **Step 4: Move remaining feature files in coherent batches**

Move one vertical slice per batch. Do not move all `components/` and `views/` files by
directory name alone. Files with cross-feature dependencies remain behind explicit public
feature exports.

- [ ] **Step 5: Add a feature-boundary test**

Add a Node test that scans imports and fails if:

- a file under `src/features/a` imports `src/features/b` internals instead of `b/index.ts`;
- `src/core` imports `src/features` or `src/app`;
- `src/shared` imports `src/features` or `src/app`.

- [ ] **Step 6: Verify**

Run:

```powershell
npm run typecheck
npm run build:web
npm run format:check
npm run test:unit
```

Expected: all pass, and `SettingsView.vue` and `CommunityView.vue` are both below 1,000 lines.

---

### Task 3: Create Rust Application And Feature Boundaries

**Files:**
- Create: `src-tauri/src/app/mod.rs`
- Create: `src-tauri/src/app/windows.rs`
- Create: `src-tauri/src/ipc/mod.rs`
- Create: `src-tauri/src/ipc/dispatch.rs`
- Create: `src-tauri/src/features/accounts/mod.rs`
- Create: `src-tauri/src/features/settings/mod.rs`
- Create: `src-tauri/src/features/library/mod.rs`
- Create: `src-tauri/src/features/connectivity/mod.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/src/ipc.rs`
- Move: `src-tauri/src/accounts.rs`, `settings.rs`, `mods.rs`, `installer.rs` only after
  their public facades are in place

**Interfaces:**
- Consumes: existing public module functions and `invoke` handlers.
- Produces: `src-tauri/src/app/mod.rs::run()` and a thin `lib.rs`.
- Produces: `src-tauri/src/ipc/dispatch.rs` with ordered handler composition. The dispatcher
  calls feature handlers first, then a documented legacy fallback.
- Produces: feature modules that expose `pub fn invoke(...)` when they own channels.

- [ ] **Step 1: Add target module declarations without moving code**

Create `app`, `ipc`, and `features` modules with `pub use` shims to existing modules. Compile
before changing behavior.

```rust
// src-tauri/src/app/mod.rs
pub use crate::run as run_app;
```

- [ ] **Step 2: Extract window lifecycle**

Move the builder window setup and splash creation from `lib.rs` into `app/windows.rs`. Keep
the hotkey, gate, and event shutdown behavior unchanged. `lib.rs` remains the crate entry
that calls `app::run()`.

- [ ] **Step 3: Extract IPC dispatcher**

Move the ordered subsystem dispatch list from `ipc.rs` into `ipc/dispatch.rs`. Preserve every
handler's relative order.

- [ ] **Step 4: Move two complete feature modules**

Move `accounts.rs` and `settings.rs` into feature packages with compatibility shims at the
old paths. Settings depends on download limit validation; keep that dependency explicit
through `infra` or a shared contract rather than importing a feature from infrastructure.

- [ ] **Step 5: Split `mods.rs` and `installer.rs` by responsibility**

Split only pure parsing, validation, and file-format helpers first. Suggested boundaries:
mod metadata, dependency planning, local install, remote install, and installer probing.
Keep the public `invoke` surface and async task behavior unchanged.

- [ ] **Step 6: Verify**

Run:

```powershell
cargo fmt --all -- --check
cargo check --lib
cargo test --lib
```

Expected: formatting, compilation, and all existing tests pass.

---

### Task 4: Add Quality Guardrails And CI

**Files:**
- Create: `.eslintrc.cjs` or `eslint.config.js`
- Create: `.github/workflows/quality.yml`
- Create: `scripts/check-architecture.mjs`
- Create: `tests/architecture.test.mjs`
- Create: `tests/fixtures/**` only when needed by the guardrail tests
- Modify: `package.json`, `README.md`, `.github/CONTRIBUTING.md`

**Interfaces:**
- Consumes: the target directory conventions and import-boundary rules from the spec.
- Produces: `npm run lint`, `npm run test:unit`, and
  `npm run check:architecture` commands.
- Produces: a CI workflow that runs all quality gates without requiring a Tauri runtime.

- [ ] **Step 1: Add ESLint**

Use ESLint's flat config with Vue and TypeScript support. Start with correctness rules and
the import boundaries needed by the spec. Do not enable formatting rules that duplicate
Prettier.

- [ ] **Step 2: Implement the architecture checker**

The checker must scan `src` and fail on:

- new files directly under `src/components`, `src/views`, or `src/composables` unless they
  are explicitly listed legacy shims;
- application imports from feature internals;
- core/shared imports from features or application shell;
- direct `window.kamucl` access outside `src/core/ipc` and the legacy `api.ts`/`shim.ts`.

Write the test first with small fixture directories, observe the test fail, then implement
the checker.

- [ ] **Step 3: Normalize formatting**

Run Prettier and `cargo fmt` only after the move is integrated, so the formatting commit is
separate from behavior changes. Preserve the project's existing 4-space TypeScript style.

- [ ] **Step 4: Add CI**

The workflow runs:

```text
npm ci
npm run format:check
npm run lint
npm run typecheck
npm run test:unit
npm run build:web
npm run check:architecture
cargo fmt --all -- --check
cargo test --lib
```

Use a cache for npm and Cargo. Do not build or sign a Tauri installer in the quality workflow.

- [ ] **Step 5: Verify**

Run every command locally. Expected: all pass; no generated files are tracked.

---

### Task 5: Integrate, Clean Up Shims, And Prove Completion

**Files:**
- Modify: only integration points and documentation
- Delete: compatibility shims whose consumers are fully migrated
- Modify the design doc if a compatibility blocker is discovered

**Interfaces:**
- Consumes: all worker outputs.
- Produces: a coherent repository with no duplicate implementations, no empty target
  directories, and no unresolved import cycles.

- [ ] **Step 1: Review every worker diff**

Check for duplicate helpers, changed IPC strings, changed error text, accidental generated
files, and formatting-only noise mixed into behavioral changes.

- [ ] **Step 2: Resolve overlapping integration files**

Integrate `package.json`, `vite.config.ts`, `src/main.ts`, `src-tauri/src/lib.rs`, and
`src-tauri/src/ipc.rs` in the coordinator workspace. Do not accept conflicting variants.

- [ ] **Step 3: Remove dead shims**

Search all old import paths and remove a shim only when no in-repository consumer imports
it.

- [ ] **Step 4: Run the full verification matrix**

```powershell
npm run format:check
npm run lint
npm run typecheck
npm run test:unit
npm run build:web
npm run check:architecture
cargo fmt --all -- --check
cargo test --lib
```

Expected: all commands pass. Capture actual output for the completion audit.

- [ ] **Step 5: Verify the final tree**

Run:

```powershell
Get-ChildItem src/app,src/core,src/features -Directory -Recurse
Get-ChildItem src-tauri/src/app,src-tauri/src/features -Directory -Recurse
rg -n "from ['\"].*(components|views)/" src/features -g '*.ts' -g '*.vue'
```

Expected: target directories contain owned modules; feature imports use public barrels or
their own package; no empty speculative directories remain.

## Self-Review

- Spec coverage: all architecture, migration, compatibility, testing, and acceptance
  requirements have an owning task.
- Type consistency: the IPC facade, app state facade, and feature barrels are defined before
  feature modules consume them.
- Correctness risk: overlapping IPC channel ownership, import cycles, loss of license
  headers, formatting churn, and behavior changes are covered by explicit verification.
- Scope: state-library replacement and protocol rewrites remain separate, behavior-sensitive
  projects.
