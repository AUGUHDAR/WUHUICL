/** VoxLink 联机控制台的共享状态：组件取件、会话事件、一次性命令。 */
import { reactive, ref } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';

/** 建房时上报的客户端标签，固定值，不给玩家改 */
export const CLIENT_TAG = 'WUHUICL';

export type VlSection = 'lobby' | 'join' | 'create' | 'room' | 'tickets' | 'settings';

export interface VlEnvelope<T = unknown> {
    ok: boolean;
    data?: T;
    code?: string;
    message?: string;
    usage?: boolean;
}

export interface VlRoomCard {
    code: string;
    name: string;
    currentPlayers?: number;
    maxPlayers?: number;
    category?: string;
    gameVersion?: string;
    loader?: string;
    clientTag?: string;
    natType?: string;
    hasPassword?: boolean;
}

export interface VlConnection {
    phase?: string;
    status?: string;
    address?: string;
    detail?: string;
}

const PHASE_LABELS: Record<string, string> = {
    p2p: '直连打洞',
    direct: '局域网直连',
    turn: 'TURN 中继',
    prelay: '玩家中继',
    fed: '互通线路',
    relay: '中继'
};

export const vl = reactive({
    /** 组件二进制是否就绪；false 时界面只出获取进度（下载在后台继续，可离开页面） */
    ready: false,
    preparing: false,
    prepareError: '',
    /** 当前安装的 release 版本号（以 GitHub/Gitee release tag 为准，非 CLI 自报构建标签） */
    version: '',
    protocol: 0,
    capabilities: [] as string[],
    /** 会话 */
    alive: false,
    state: 'idle',
    startedAt: 0,
    room: null as Record<string, unknown> | null,
    connection: null as VlConnection | null,
    logs: [] as { ts: number; level: string; text: string }[],
    lastError: '',
    /** 房号是否可能在服务端滞留（强杀后靠心跳过期） */
    roomMayLinger: false,
    /** 后台更新流水线状态（检查/下载/待替换全部静默发生，这里只做展示） */
    update: {
        phase: 'idle',
        latest: '',
        bytes: 0,
        total: null as number | null,
        progress: null as number | null,
        message: '',
        pending: '' as string | null
    } as VlUpdateState
});

export interface VlUpdateState {
    phase: 'idle' | 'checking' | 'downloading' | 'pending' | 'latest' | 'failed';
    latest?: string;
    bytes?: number;
    total?: number | null;
    progress?: number | null;
    message?: string;
    version?: string;
    lastCheckAt?: number;
    lastError?: string | null;
    pending?: string | null;
}

export const section = ref<VlSection>('lobby');

/** 建房分类标签由服务端给出，value → 中文名 */
export const categories = ref<Record<string, string>>({});

export async function loadCategories(): Promise<void> {
    const r = await call<Record<string, string>>('vlcli:lobbyCategories');
    if (r.ok && r.data && typeof r.data === 'object') categories.value = r.data as Record<string, string>;
}

/** 窗口级文件拖放：Tauri 的 drop 事件带真实路径，HTML5 dataTransfer 拿不到。 */
export function onDropPaths(handler: (paths: string[]) => void): () => void {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void getCurrentWindow().onDragDropEvent((event) => {
        if (disposed) return;
        const payload = event.payload as { type: string; paths?: string[] };
        if (payload.type === 'drop' && Array.isArray(payload.paths)) handler(payload.paths);
    }).then((u) => {
        if (disposed) u();
        else unlisten = u;
    });
    return () => {
        disposed = true;
        if (unlisten) unlisten();
    };
}

/** 大厅点加入时暂存房号，加入页读一次就清空 */
export const pendingJoinCode = ref('');
/** 大厅直加失败时带去加入页的原因（如实际有密码、门控拦截） */
export const pendingJoinError = ref('');

let bound = false;

/** 把后台更新流水线的事件折进展示状态（检查/下载/待替换全程静默，界面只读这里）。 */
function onUpdateState(data: Record<string, unknown>): void {
    const u = vl.update as unknown as Record<string, unknown>;
    for (const key of ['phase', 'latest', 'bytes', 'total', 'progress', 'message', 'version', 'lastCheckAt', 'lastError', 'pending']) {
        if (data[key] !== undefined) u[key] = data[key];
    }
    if (typeof data.version === 'string' && data.version) vl.version = data.version;
}

function onEvent(payload: unknown): void {
    const ev = payload as { kind?: string; data?: Record<string, unknown> };
    const data = ev.data || {};
    switch (ev.kind) {
        case 'session':
            if (typeof data.state === 'string') vl.state = data.state;
            if (data.room) vl.room = data.room as Record<string, unknown>;
            if (data.state === 'idle') {
                vl.room = null;
                vl.connection = null;
            }
            break;
        case 'conn':
            vl.connection = {
                phase: String(data.phase || ''),
                status: String(data.status || ''),
                address: data.address ? String(data.address) : '',
                detail: data.detail ? String(data.detail) : ''
            };
            break;
        case 'log': {
            const level = String(data.level || 'info');
            const msg = String(data.msg || '');
            vl.logs.push({ ts: Number(data.ts || 0), level, text: msg });
            if (vl.logs.length > 200) vl.logs.splice(0, vl.logs.length - 200);
            break;
        }
        case 'error':
            vl.lastError = String(data.message || '联机操作未完成');
            break;
        case 'update':
            onUpdateState(data);
            break;
        case 'closed':
            vl.alive = false;
            vl.state = 'idle';
            vl.room = null;
            vl.connection = null;
            break;
        default:
            break;
    }
}

/** 一次性命令：永远返回信封，业务错误不抛异常。 */
export async function vlCall<T = unknown>(
    channel: string,
    payload: Record<string, unknown> = {}
): Promise<VlEnvelope<T>> {
    try {
        const r = (await window.kamucl.invoke(channel, payload)) as VlEnvelope<T>;
        return r && typeof r.ok === 'boolean' ? r : { ok: false, code: 'BAD_REPLY', message: '联机组件返回异常' };
    } catch (e) {
        return { ok: false, code: 'LAUNCHER_ERROR', message: e instanceof Error ? e.message : String(e) };
    }
}

export const call = vlCall;
export const ensure = vlEnsure;
export const refresh = vlRefreshStatus;
export const host = vlHost;
export const join = vlJoin;
export const stop = vlStop;

/** 准备联机组件：后端本地判定毫秒级返回；检查与下载全在后台流水线，界面永不等网络。 */
export async function vlEnsure(force = false): Promise<boolean> {
    if (vl.ready && !force) return true;
    if (vl.preparing) return false;
    vl.preparing = true;
    vl.prepareError = '';
    const r = await vlCall<{ ready?: boolean; version?: string }>('vlcli:ensure', { force });
    if (!r.ok) {
        const err = (r.data as { error?: string } | undefined)?.error;
        vl.prepareError = r.message || err || '联机组件不可用';
    } else {
        const d = (r.data || {}) as { ready?: boolean; version?: string };
        vl.version = String(d.version || vl.version || '');
        vl.ready = Boolean(d.ready);
    }
    if (!vl.ready) {
        // 还没有二进制：读一次后台流水线状态，界面显示下载进度（可离开页面，回来接着看）
        const s = await vlCall<VlUpdateState>('vlcli:updateState');
        if (s.ok && s.data) onUpdateState(s.data as unknown as Record<string, unknown>);
    } else {
        void fillCapabilities();
        // 已就绪也同步一次更新状态：后台可能在用户离开时完成了升级，版本显示要追上
        const s = await vlCall<VlUpdateState>('vlcli:updateState');
        if (s.ok && s.data) onUpdateState(s.data as unknown as Record<string, unknown>);
    }
    vl.preparing = false;
    return vl.ready;
}

/** 协议号与能力清单：纯本地探测，异步补齐；不阻塞进入，也不覆盖 release 版本号。 */
async function fillCapabilities(): Promise<void> {
    const cap = await vlCall<Record<string, unknown>>('vlcli:capabilities');
    if (!cap.ok) return;
    const data = (cap.data || {}) as Record<string, unknown>;
    vl.protocol = Number(data.protocolVersion || 0);
    vl.capabilities = Array.isArray(data.capabilities) ? (data.capabilities as string[]) : [];
}

/** 手动触发一次后台检查（设置页「检查更新」）；结果经 update 事件回流，不在这里等。 */
export async function vlCheckUpdate(): Promise<void> {
    await vlCall('vlcli:ensure', { force: true });
    const s = await vlCall<VlUpdateState>('vlcli:updateState');
    if (s.ok && s.data) onUpdateState(s.data as unknown as Record<string, unknown>);
}

export async function vlRefreshStatus(): Promise<void> {
    const r = await vlCall<{ alive?: boolean; snapshot?: Record<string, unknown> }>('vlcli:status');
    const snap = (r.data?.snapshot || {}) as Record<string, unknown>;
    vl.alive = Boolean(r.data?.alive);
    vl.startedAt = Number(snap.startedAt || 0);
    if (typeof snap.state === 'string') vl.state = snap.state;
    vl.room = (snap.room as Record<string, unknown>) || null;
    vl.connection = (snap.connection as VlConnection) || null;
    vl.lastError = String(snap.lastError || '');
    if (Array.isArray(snap.logs)) vl.logs = snap.logs as typeof vl.logs;
}

export function vlHost(params: Record<string, unknown>): Promise<VlEnvelope> {
    return vlCall('vlcli:host', params);
}

export function vlJoin(code: string, password: string, modsync = 'gate'): Promise<VlEnvelope> {
    return vlCall('vlcli:join', { code, password, modsync });
}

export async function vlStop(): Promise<void> {
    const r = await vlCall<{ roomMayLinger?: boolean }>('vlcli:stop');
    vl.roomMayLinger = Boolean(r.data?.roomMayLinger);
    vl.alive = false;
    vl.state = 'idle';
    vl.room = null;
    vl.connection = null;
}

export function phaseLabel(phase?: string): string {
    if (!phase) return '等待中';
    return PHASE_LABELS[phase] || phase;
}

/** 连接结果：只有 conn:state 给出地址才算连上。 */
export function connectedAddress(): string {
    const c = vl.connection;
    return c && c.status === 'success' && c.address ? c.address : '';
}

export function useVoxlink() {
    if (!bound) {
        bound = true;
        window.kamucl.on('vlcli:event', onEvent);
    }
    return { vl, section, ensure: vlEnsure, call: vlCall, host: vlHost, join: vlJoin, stop: vlStop, refresh: vlRefreshStatus };
}
