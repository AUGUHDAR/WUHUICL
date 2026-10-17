/**
 * Stable renderer IPC surface. Existing feature modules still import the legacy
 * `src/api.ts` facade, which delegates to this module during the migration.
 */
export * from '../../api';
export * from './bridge';
export * from './skinRevision';
