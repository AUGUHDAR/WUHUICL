/** Compatibility shim; idle lifecycle is owned by `src/core/runtime/idleTrim.ts`. */
export {
    registerIdleReleasable,
    startIdleTrim,
    type IdleReleasable
} from './core/runtime/idleTrim';
