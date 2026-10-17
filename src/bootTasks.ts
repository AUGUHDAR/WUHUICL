/** Compatibility shim; boot lifecycle is owned by `src/core/runtime/index.ts`. */
export {
    beginBootTask,
    sealBootTasks,
    trackBootTask,
    waitForBootTasks
} from './core/runtime';
