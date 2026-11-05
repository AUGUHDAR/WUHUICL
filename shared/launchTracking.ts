import type { LaunchState } from './types';
import { instanceKey } from './modCompatibility';

export interface LaunchTracking {
    launchState: LaunchState | null;
    launchStates: Record<string, LaunchState>;
    launchingVersionId: string;
    launchingFolder: string;
}
/** 已退出/出错的启动记录保留上限（整次会话累计启动不再无限增长） */
const EXITED_LAUNCH_STATES_LIMIT = 32;
function pruneExitedLaunchStates(states: Record<string, LaunchState>): void {
    const exited = Object.keys(states).filter(
        (k) => states[k].status === 'exited' || states[k].status === 'error'
    );
    for (const k of exited.slice(0, Math.max(0, exited.length - EXITED_LAUNCH_STATES_LIMIT))) {
        delete states[k];
    }
}

export function trackLaunchState(state: LaunchTracking, incoming: LaunchState): boolean {
    const focused =
        !incoming.versionId ||
        (incoming.versionId === state.launchingVersionId &&
            (incoming.folder ?? '') === state.launchingFolder &&
            (!incoming.launchId ||
                !state.launchState?.launchId ||
                incoming.launchId === state.launchState.launchId));
    if (incoming.versionId) {
        const key = instanceKey({ id: incoming.versionId, folder: incoming.folder ?? '' });
        if (incoming.launchId) delete state.launchStates[key]; // replace optimistic renderer preparation
        state.launchStates[incoming.launchId ? `${key}#${incoming.launchId}` : key] = incoming;
        pruneExitedLaunchStates(state.launchStates);
    }
    if (incoming.status === 'launching' || focused || !state.launchState) {
        let next = incoming;
        if (incoming.status === 'exited' || incoming.status === 'error') {
            const active = Object.values(state.launchStates).filter(
                (s) => s.status === 'running' || s.status === 'launching'
            );
            next =
                [...active].reverse().find((s) => s.status === 'launching') ??
                active.at(-1) ??
                incoming;
        }
        state.launchState = next;
        if (next.versionId) {
            state.launchingVersionId = next.versionId;
            state.launchingFolder = next.folder ?? '';
        }
    }
    return focused;
}
export function instanceLaunchBusy(
    states: Record<string, LaunchState>,
    id: string,
    folder?: string
): boolean {
    return Object.values(states).some(
        (s) => s.versionId === id && (s.folder ?? '') === (folder ?? '') && s.status === 'launching'
    );
}
