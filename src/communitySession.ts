import type { CommunityQuery, CommunityResult } from '@shared/types';
import type { CommunityVersionSelection } from './communityVersionSelection';

/** Route-local, memory-only history: leaving the page releases its DOM and dialogs. */
export interface CommunitySession {
    query: Omit<CommunityQuery, 'offset' | 'limit'>;
    tab: 'browse' | 'favorites';
    favoriteSearch: string;
    versionInput: string;
    versionSelection?: CommunityVersionSelection;
    results: CommunityResult[];
    searched: boolean;
    error: string;
    offset: number;
    hasMore: boolean;
    page: number;
    total: number;
    warnings: string[];
    scrollTop: number;
    topKeyword: string;
    interrupted: boolean;
}
let last: CommunitySession | undefined;
/** 快照保留的结果条数上限：无限滚动可以一直翻页，但跨页面驻留的快照不能跟着无限涨 */
const COMMUNITY_SESSION_RESULTS_LIMIT = 400;
export function saveCommunitySession(value: CommunitySession): void {
    const clone = JSON.parse(JSON.stringify(value)) as CommunitySession;
    if (clone.results.length > COMMUNITY_SESSION_RESULTS_LIMIT) {
        clone.results.length = COMMUNITY_SESSION_RESULTS_LIMIT;
    }
    last = clone;
}
export function readCommunitySession(): CommunitySession | undefined {
    return last ? structuredClone(last) : undefined;
}
export function clearCommunitySession(): void {
    last = undefined;
}
