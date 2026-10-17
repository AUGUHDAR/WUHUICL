# Frontend Feature Packages

Each directory is a vertical feature package with a public `index.ts`. UI and data imports from
another feature must go through that barrel; a feature may import its own `ui/` and `data/`
modules directly.

## Legacy file ownership

| Legacy source | Feature owner | Migration state |
| --- | --- | --- |
| `src/views/SettingsView.vue` | `settings` | moved, compatibility shim |
| `src/settingsUpdates.ts` | `settings` | moved, compatibility shim |
| `src/components/AppearanceThemeCard.vue` | `settings` / appearance | moved, compatibility shim |
| `src/components/UpdateSources.vue` | `settings` / updates | moved, compatibility shim |
| `src/components/MemoryOrganizer.vue` | `settings` | moved, compatibility shim |
| `src/components/HomeLayoutEditor.vue` | `appearance` | moved, compatibility shim |
| `src/components/UpdateDialogShell.vue` | `updates` | moved, compatibility shim |
| `src/components/UpdateModal.vue` | `updates` | moved, compatibility shim |
| `src/components/ThirdPartyNotices.vue` | `settings` / updates | moved, compatibility shim |
| `src/views/CommunityView.vue` | `community` | moved, compatibility shim |
| `src/communitySession.ts` | `community` | moved, compatibility shim |
| `src/communityVersionSelection.ts` | `community` | moved, compatibility shim |
| `src/communityDownloadQueue.ts` | `community` | moved, compatibility shim |
| `src/useCommunityDownloadQueue.ts` | `community` | moved, compatibility shim |
| `src/modFavorites.ts` | `community` | moved, compatibility shim |
| `src/components/Community*.vue` | `community` | moved, compatibility shim |
| `src/views/HomeView.vue` | `home` | moved, compatibility shim |
| `src/views/GameView.vue` | `instances` | moved, compatibility shim |
| `src/views/ModsView.vue`, `PacksView.vue`, `ShadersView.vue` | `mods` | moved, compatibility shim |
| `src/views/FriendConnectView.vue`, `src/components/connection/**` | `connectivity` | moved, compatibility shim |
| `src/views/SkinsView.vue`, skin-only components and helpers | `skins` | moved, compatibility shim |
| `src/views/AccountsView.vue` | `accounts` | moved, compatibility shim |
| `src/components/Update*.vue`, update helpers | `updates` | moved, compatibility shim |

Cross-feature primitives such as `Avatar`, `ConfirmModal`, `ContentSkeleton`, `MarqueeText`,
`SelectMenu`, and `UiGlyph` remain in `src/components` until the shared UI package owns them.
