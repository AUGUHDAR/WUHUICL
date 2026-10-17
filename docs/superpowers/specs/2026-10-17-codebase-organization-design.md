# WUHUICL Code Organization Design

**Status:** Implementation baseline

**Date:** 2026-10-17

## Goal

Make the repository's physical structure describe its actual architecture. A contributor
should be able to find a feature, identify its view, state, IPC boundary, and backend
implementation without searching a flat directory or reading a multi-thousand-line file.

The refactor must preserve all existing IPC channel names, payload shapes, UI copy, visual
behavior, and data formats. It is a structural migration, not a feature rewrite.

## Evidence

The current repository is a Vue 3 + TypeScript + Vite frontend with a Tauri 2 + Rust backend.
It already contains empty feature-first directories beside the populated legacy layout:

- `src/app`, `src/core`, and `src/features` exist, but all 131 frontend code files remain in
  legacy `components`, `views`, `composables`, and root-level modules.
- `src-tauri/src/app`, `features`, `infra`, `ipc`, and `shared` exist, but the 62 Rust source
  files remain in a flat module layout plus the specialized `vl`, `vlcli`, and `versions`
  directories.
- `src/App.vue` is 3,578 lines; `GameView.vue` is 3,617 lines; `SettingsView.vue` is 3,145
  lines. Rust's `mods.rs` is 5,264 lines and `installer.rs` is 4,094 lines.
- `src/api.ts` is a 572-line IPC facade imported by nearly every feature.
- `src/store.ts` combines application, navigation, settings, account, install, launch, and
  toast state in one reactive singleton.
- Type checking and frontend production build pass. 158 Rust tests pass; 2 live-network
  tests are intentionally ignored.
- `cargo fmt --check` fails broadly and `prettier --check` fails in seven VoxLink files.
- There is no frontend test runner, ESLint configuration, automated quality workflow, or
  architecture guardrail.

## Architecture

### Frontend

Use a feature-first layout with explicit shared and application boundaries:

```text
src/
├── app/
│   ├── bootstrap/       # create application, install global services
│   ├── composables/     # shell-level composables
│   ├── overlays/        # application-level dialogs and notifications
│   ├── shell/           # root shell and window chrome
│   └── state/           # application session and shell state
├── core/
│   ├── assets/          # managed asset URL helpers
│   ├── ipc/             # the single Tauri adapter and typed channel facade
│   ├── runtime/         # boot tasks and idle lifecycle
│   ├── text/            # text rendering helpers
│   └── ui/              # design-system primitives and global styles
├── features/
│   └── <feature>/
│       ├── data/        # feature API adapters, persistence, and state
│       ├── ui/          # views, feature composables, and components
│       └── vendor/      # only where a feature owns vendored rendering code
└── shared/              # cross-feature domain utilities and UI primitives
```

Rules:

1. Application shell code may import features. A feature may import `core`, `shared`, and its
   own internals. It must not import the application shell.
2. Cross-feature imports must use another feature's public `index.ts`, not its internal
   `ui/` or `data/` files.
3. Feature UI must not call `window.kamucl` directly. Tauri access belongs in `core/ipc`.
4. Core must not import features or application shell modules.
5. Shared modules must remain framework-light and must not import feature code.
6. A view should compose feature components and composables. Large view-only template
   sections move into feature UI components with explicit props and emits.

The application continues to use the existing lightweight `reactive` store during the first
migration pass. Splitting the store into feature state slices is a behavior-sensitive
follow-up; the first pass moves and re-exports it through a stable facade so consumers do not
change twice.

### Backend

Use a feature-first Rust layout with shared infrastructure:

```text
src-tauri/src/
├── app/                  # Tauri builder, window lifecycle, boot gate
├── features/
│   ├── accounts/
│   ├── community/
│   ├── downloads/
│   ├── instances/
│   ├── launch/
│   ├── library/          # mods, modpacks, worlds, projections, recordings
│   ├── settings/
│   └── ...
├── infra/                # HTTP, task transport, persistence primitives
├── ipc/                  # channel routing and application service composition
├── shared/               # domain formats and cross-feature contracts
└── vendor/               # VoxLink and its CLI bridge
```

Rules:

1. `app` composes features and infrastructure; it does not own feature behavior.
2. Feature modules expose a module-level `invoke` handler where they currently own IPC
   channels. The dispatcher composes those handlers and retains a temporary legacy fallback
   until all channels have moved.
3. Features may use `shared` and `infra`; they must not depend on the `app` module.
4. Infrastructure may not import features. Cross-feature calls go through explicit shared
   contracts or application-level composition.
5. Existing public function names and IPC behavior remain stable during moves. Re-exports
   are allowed only as migration shims, and each shim states its removal condition.
6. Large modules are split by responsibility, not by mechanical line count. Pure helpers and
   protocol/data operations move first because they carry the lowest behavior risk.

## Migration Strategy

Use a strangler migration rather than a big-bang rewrite:

1. Establish target directories, compatibility facades, linting, formatting, and CI.
2. Move application shell and cross-cutting core modules while preserving import paths
   through re-exports.
3. Move feature modules and split views/components, one feature at a time.
4. Move and split Rust feature modules, verifying each package with `cargo check`, `cargo
   test`, and formatting.
5. Remove compatibility facades only after all in-repository consumers use the public feature
   API.

No compatibility shim may remain "forever" without an owner and removal condition. The
steady-state layout is the target tree above.

## Error Handling And Compatibility

- IPC channel strings and event names are an external contract. Do not rename them.
- Tauri command names, argument order, returned JSON fields, and error text must not change.
- Local storage keys and persisted file formats must not change.
- Feature moves must preserve `SPDX-License-Identifier` headers.
- If a module cannot be moved without changing behavior, keep it in place and document the
  blocker instead of shipping a speculative translation.

## Testing And Guardrails

Each implementation slice must pass relevant checks:

- `npm run typecheck`
- `npm run build:web`
- `npm run format:check`
- `cargo fmt --all -- --check`
- `cargo test --lib`

New guardrails:

- ESLint with Vue and TypeScript rules.
- A small Node-based architecture checker that validates forbidden dependency directions and
  prevents new source files in legacy directories.
- An automated CI workflow that runs frontend and Rust quality gates on pull requests.
- At least one frontend unit test setup for pure logic during the migration, with tests
  covering the new architecture checker and key migrated facade behavior.

## Non-Goals

- Redesigning UI or changing user-visible copy.
- Rewriting the state-management library.
- Renaming IPC channels or public Tauri commands.
- Replacing the networking or download engines.
- Moving generated assets, icons, or vendored binaries.
- Rewriting the VoxLink protocol modules in this pass.

## Acceptance Criteria

1. Target directories contain real code; legacy directories are no longer the default home
   for new source files.
2. At least the application shell, core IPC facade, and two representative feature
   packages have moved to the target layout with stable imports.
3. Rust has at least two complete feature packages moved behind a composition layer, with
   unchanged channel behavior and tests passing.
4. Large root-level frontend modules are split into focused modules or clearly documented
   migration shims with a target owner.
5. Formatting and the newly introduced lint/architecture checks pass.
6. A CI workflow enforces type checking, formatting, linting, frontend build, and Rust tests.
7. No uncommitted generated output or unrelated user changes are introduced.
