// feedbackApi.ts — 「反馈与建议」的 IPC 封装（通道合同见 src-tauri/src/feedback.rs）。
import { invoke } from './fbInvoke';

export interface FBIdentity {
    provider: string;
    uid: string;
    name: string;
}
export interface FBKind {
    key: string;
    name: string;
    color: string;
    logMode: 'required' | 'optional' | 'none';
}
export interface FBStatus {
    key: string;
    name: string;
    color: string;
}
export interface FBMeta {
    kinds: FBKind[];
    statuses: FBStatus[];
    loggedIn: boolean;
    identity: FBIdentity | null;
    tickets: number;
}
export interface FBAttachment {
    fid: string;
    name: string;
    size: number;
}
export interface FBIssueRow {
    num: number;
    title: string;
    time: number;
    kind: string;
    kindName: string;
    kindColor: string;
    status: string;
    statusName: string;
    statusColor: string;
    author: string;
    authorUid: string;
    comments: number;
    attachments: number;
}
export interface FBComment {
    id: string;
    author: string;
    authorUid: string;
    from: string;
    time: number;
    text: string;
    attachments?: FBAttachment[];
    mine?: boolean;
}
export interface FBIssueDetail {
    num: number;
    title: string;
    time: number;
    body: string;
    kind: string;
    kindName: string;
    kindColor: string;
    logMode: string;
    status: string;
    author: string;
    authorUid: string;
    attachments: FBAttachment[];
    comments: FBComment[];
    context?: Record<string, unknown>;
    canComment: boolean;
    canDelete: boolean;
    fromTicket?: string;
}
export interface FBTicketLocal {
    id: string;
    secret: string;
    title?: string;
    kind?: string;
    timeMs: number;
    lastTimeMs: number;
    replyCount: number;
    hasUnread: boolean;
}
export interface FBTicketDetail {
    id: string;
    time: number;
    kind: string;
    title: string;
    description: string;
    attachments: FBAttachment[];
    messages: { id: string; from: string; time: number; text: string; attachments?: FBAttachment[] }[];
    deleted: boolean;
    publishedTo: number;
}

export const fbMeta = () => invoke<FBMeta>('feedback:meta');
/** 第一步：取配对码并打开系统浏览器；后台线程随后轮询领取，结果经 event:feedbackLoginDone 回来。 */
export const fbLogin = () => invoke<{ code: string; verifyUrl: string }>('feedback:login');
export const fbLoginCancel = () => invoke<{ ok: boolean }>('feedback:loginCancel');
export const fbLogout = () => invoke<{ ok: boolean }>('feedback:logout');
export const fbIssuesList = (q: {
    kind?: string;
    status?: string;
    q?: string;
    page?: number;
    size?: number;
    mine?: boolean;
}) => invoke<{ total: number; page: number; size: number; issues: FBIssueRow[] }>('feedback:issues:list', q);
export const fbIssueGet = (num: number) => invoke<FBIssueDetail>('feedback:issue:get', num);
export const fbIssueSubmit = (a: {
    title: string;
    body: string;
    kind: string;
    withLogs: boolean;
    files: string[];
}) => invoke<{ num: number; url: string }>('feedback:issue:submit', a);
export const fbIssueComment = (a: { num: number; text: string; files?: string[] }) =>
    invoke<{ ok: boolean }>('feedback:issue:comment', a);
export const fbIssueRetract = (num: number, msg: string) =>
    invoke<{ ok: boolean }>('feedback:issue:retract', { num, msg });
export const fbIssueDelete = (num: number) => invoke<{ ok: boolean }>('feedback:issue:delete', { num });
export const fbTicketsList = () => invoke<FBTicketLocal[]>('feedback:tickets:list');
export const fbTicketsPoll = () =>
    invoke<{ tickets: { id: string; deleted: boolean; hasUnread: boolean; replyCount: number; lastTime: number }[]; removed: string[] }>(
        'feedback:tickets:poll'
    );
export const fbTicketSubmit = (a: { title: string; description: string; kind: string; withLogs: boolean }) =>
    invoke<{ id: string; secret: string }>('feedback:ticket:submit', a);
export const fbTicketDetail = (id: string) => invoke<FBTicketDetail>('feedback:ticket:detail', id);
export const fbTicketReply = (a: { id: string; text: string; withLogs?: boolean }) =>
    invoke<{ ok: boolean }>('feedback:ticket:reply', a);
export const fbTicketViewed = (id: string) => invoke<{ ok: boolean }>('feedback:ticket:viewed', id);
export const fbTicketDelete = (id: string) => invoke<{ ok: boolean }>('feedback:ticket:delete', id);
export const fbTicketRetract = (id: string, msg: string) =>
    invoke<{ ok: boolean }>('feedback:ticket:retract', { id, msg });
export const fbAttachment = (kind: 'issue' | 'ticket', owner: string, fid: string) =>
    invoke<{ path: string; size: number }>('feedback:attachment', { kind, owner, fid });
export const fbLogsPreview = () =>
    invoke<{ name: string; size: number }[]>('feedback:logs:preview');
export const fbPickFiles = () => invoke<string[]>('feedback:pickFiles');
