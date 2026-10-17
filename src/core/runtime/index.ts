/**
 * Core runtime lifecycle helpers. Legacy root imports are preserved as re-export
 * shims while feature state and view modules migrate to the core boundary.
 */
export {
    beginBootTask,
    sealBootTasks,
    trackBootTask,
    waitForBootTasks
} from './bootTasks';
export {
    registerIdleReleasable,
    startIdleTrim,
    type IdleReleasable
} from './idleTrim';
