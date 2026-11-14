<script setup lang="ts">
import { terracottaRole } from '@shared/uiPresentation';
import { computed, onMounted, onUnmounted, ref } from 'vue';
import UiGlyph from '../UiGlyph.vue';
import TabPills from '../TabPills.vue';
import EmptyState from '../EmptyState.vue';
import ConnectionPanel from './ConnectionPanel.vue';
import ConnectionStatus from './ConnectionStatus.vue';
import { toast } from '../../store';
import { copyText } from '../../api';

interface TcStatus {
    phase: 'idle' | 'downloading' | 'starting' | 'hosting' | 'joining' | 'ready';
    room?: string;
    url?: string;
    stateRaw?: string;
    error?: string;
    downloaded?: number;
    total?: number;
    binaryReady: boolean;
    running: boolean;
    toolVersion?: string;
    binaryPath?: string;
}

/** 分区状态跨页面切换保留（与 VoxLink 的 section 同模式） */
type TcTab = 'create' | 'join' | 'session' | 'tool';
const tab = ref<TcTab>('create');
const NAV: Array<{ key: TcTab; label: string }> = [
    { key: 'create', label: '创建房间' },
    { key: 'join', label: '加入房间' },
    { key: 'session', label: '当前会话' },
    { key: 'tool', label: '工具' }
];

const status = ref<TcStatus | null>(null);
const busy = ref(false);
const error = ref('');
const logs = ref<Array<{ ts: string; text: string }>>([]);
const mode = ref<'host' | 'join'>('host');
const roomCode = ref('');
const playerName = ref('');

/* 分区 tab（TabPills 收编）：会话进行中显示红点 */
const tabItems = computed(() =>
    NAV.map((item) => ({
        value: item.key,
        label: item.label,
        dot: item.key === 'session' && inSession.value
    }))
);
/* 下载进度（全局 .progress-bar 收编，替代原生 <progress>） */
const downloadPercent = computed(() => {
    const s = status.value;
    if (!s || !s.total) return null;
    return Math.min(100, Math.round(((s.downloaded || 0) / s.total) * 100));
});

/** 官方房间码格式：U/ 前缀 + 四段各 4 位（burningtnt/Terracotta） */
const ROOM_CODE_RE = /^U\/[A-Z0-9]{4}(-[A-Z0-9]{4}){3}$/;
const joinCodeValid = computed(() => ROOM_CODE_RE.test(roomCode.value.trim().toUpperCase()));
/** 只有 ready 且拿到本地地址才算「已连接」；joining/hosting 只是进行中 */
const connected = computed(
    () => !!status.value && status.value.phase === 'ready' && !!status.value.url
);
const hosting = computed(() => terracottaRole(status.value) === 'host');
const toolReady = computed(() => !!status.value?.binaryReady);
/** 下载中由工具分区全权展示进度 */
const downloading = computed(() => status.value?.phase === 'downloading');
/** 会话进行中（不含下载阶段） */
const inSession = computed(() => {
    const p = status.value?.phase;
    return (
        p === 'starting' || p === 'hosting' || p === 'joining' || p === 'ready' ||
        (!!status.value?.running && !downloading.value)
    );
});
const busySession = computed(
    () => !!status.value && ['starting', 'hosting', 'joining'].includes(status.value.phase)
);
/** 创建/加入不可用的原因：为空表示可用 */
const opsBlockReason = computed(() => {
    if (!toolReady.value) return '先下载并校验陶瓦工具，才能创建或加入房间。';
    if (inSession.value) return '已有进行中的会话；先关闭房间或断开，再创建或加入。';
    return '';
});

interface Step {
    label: string;
    state: 'done' | 'active' | 'pending';
    detail?: string;
}
/** 阶段条：由 tc:status 的真实 phase / stateRaw 驱动 */
const steps = computed<Step[]>(() => {
    const s = status.value;
    if (!s || (s.phase === 'idle' && !s.running)) return [];
    const phase = s.phase;
    const stepState = (active: boolean, done: boolean): 'done' | 'active' | 'pending' =>
        done ? 'done' : active ? 'active' : 'pending';
    return [
        {
            label: '准备官方工具',
            state: stepState(phase === 'downloading', s.binaryReady),
            detail: s.binaryReady ? '已就绪（SHA-256 校验通过）' : '等待下载'
        },
        {
            label: '启动陶瓦引擎',
            state: stepState(phase === 'starting', s.running),
            detail: s.running ? '运行中' : undefined
        },
        {
            label: hosting.value ? '创建房间' : '加入房间',
            state: stepState(phase === 'hosting' || phase === 'joining', !!s.room || !!s.url),
            detail: s.stateRaw ? `引擎状态 ${s.stateRaw}` : undefined
        },
        {
            label: hosting.value ? '房间就绪' : '连接就绪',
            state: stepState(false, phase === 'ready'),
            detail:
                phase === 'ready' ? (hosting.value ? '房间码已生成' : '本地地址已生成') : undefined
        }
    ];
});

/** 消敏：远程地址不进日志（本地 127.0.0.1:端口 除外） */
function sanitizeLog(text: string): string {
    return text
        .replace(/(\d{1,3}\.){3}\d{1,3}:\d+/g, (m) => (m.startsWith('127.0.0.1') ? m : '***:***'))
        .replace(/(\d{1,3}\.){3}\d{1,3}/g, (m) => (m === '127.0.0.1' ? m : '***'));
}

const logViewport = ref<HTMLElement | null>(null);
function pushLog(text: string): void {
    const d = new Date();
    const ts = [d.getHours(), d.getMinutes(), d.getSeconds()]
        .map((n) => String(n).padStart(2, '0'))
        .join(':');
    logs.value.push({ ts, text: sanitizeLog(text) });
    if (logs.value.length > 300) logs.value.shift();
    requestAnimationFrame(() => {
        const el = logViewport.value;
        if (el) el.scrollTop = el.scrollHeight;
    });
}
async function copyLogs(): Promise<void> {
    const text = logs.value.map((l) => `[${l.ts}] ${l.text}`).join('\n');
    toast((await copyText(text)) ? '日志已复制' : '复制失败', 'info');
}

let offEvent: (() => void) | undefined;

function onEvent(payload: { type: string; data: unknown }): void {
    if (payload.type === 'status') {
        status.value = {
            toolVersion: status.value?.toolVersion,
            binaryPath: status.value?.binaryPath,
            binaryReady: status.value?.binaryReady ?? false,
            running: status.value?.running ?? false,
            ...(payload.data as TcStatus)
        };
        return;
    }
    if (payload.type === 'log') {
        const d = payload.data as { level: string; msg: string };
        if (d?.msg) pushLog(`[${d.level}] ${d.msg}`);
    } else if (payload.type === 'error') {
        error.value = String(payload.data);
    } else if (payload.type === 'stopped') {
        void refresh();
    } else if (payload.type === 'ready') {
        void refresh();
    }
}

async function refresh(): Promise<void> {
    try {
        status.value = await window.kamucl.invoke('tc:status');
        if (status.value?.error) error.value = status.value.error;
        const role = terracottaRole(status.value);
        if (role !== 'none') mode.value = role === 'host' ? 'host' : 'join';
    } catch {
        /* 窗口关闭 */
    }
}

async function install(): Promise<void> {
    busy.value = true;
    error.value = '';
    try {
        await window.kamucl.invoke('tc:install');
        toast('陶瓦工具已下载并通过校验', 'success');
        tab.value = 'create';
    } catch (e) {
        error.value = (e as Error).message.replace(
            /^Error invoking remote method '[^']*': (Error: )?/,
            ''
        );
    } finally {
        busy.value = false;
        await refresh();
    }
}
async function cancelInstall() {
    await window.kamucl.invoke('tc:cancel-install');
}
async function start(): Promise<void> {
    if (!status.value?.binaryReady) return;
    busy.value = true;
    error.value = '';
    try {
        const result = await window.kamucl.invoke<TcStatus>('tc:start', {
            mode: mode.value,
            code: roomCode.value.trim().toUpperCase(),
            playerName: playerName.value || undefined
        });
        status.value = result;
        tab.value = 'session';
    } catch (e) {
        error.value =
            (e as Error).message?.replace(
                /^Error invoking remote method '[^']*': (Error: )?/,
                ''
            ) ?? '操作失败';
    } finally {
        busy.value = false;
        void refresh();
    }
}
async function stop(): Promise<void> {
    busy.value = true;
    try {
        await window.kamucl.invoke('tc:stop');
    } catch {
        /* 忽略 */
    } finally {
        busy.value = false;
        void refresh();
    }
}
function joinNow(): void {
    if (!joinCodeValid.value || busy.value || status.value?.running) return;
    mode.value = 'join';
    void start();
}
async function copy(value?: string | null): Promise<void> {
    if (value) toast((await copyText(value)) ? '已复制' : '复制失败', 'info');
}

onMounted(async () => {
    offEvent = window.kamucl.on('tc:event', onEvent);
    await refresh();
    if (!toolReady.value) tab.value = 'tool';
});
onUnmounted(() => {
    offEvent?.();
});
</script>

<template>
    <div data-ui="TerracottaPanel:8b1e2f3973ef" class="tc-console">
        <!-- 分区导航（TabPills 收编：与全站分段单选同形态） -->
        <TabPills
            v-model="tab"
            :items="tabItems"
            aria-label="陶瓦分区"
            data-ui="tc:nav"
        />

        <p data-ui="TerracottaPanel:1861deb0662b" v-if="error" class="connection-error" role="alert">
            {{ error }}
        </p>

        <div class="tc-tabbody" :key="tab">
            <!-- 创建房间 -->
            <template v-if="tab === 'create'">
                <div v-if="opsBlockReason" class="connection-notice tc-notice" data-ui="tc:create-blocked">
                    <UiGlyph name="warning" :size="19" />
                    <div>
                        <strong>{{ !toolReady ? '先下载陶瓦工具' : '已有进行中的会话' }}</strong>
                        <p>{{ opsBlockReason }}</p>
                        <div class="connection-actions tc-notice-actions">
                            <button v-if="!toolReady" class="btn btn-gold btn-sm" @click="tab = 'tool'">
                                去下载工具
                            </button>
                            <button v-else class="btn btn-gold btn-sm" @click="tab = 'session'">
                                查看当前会话
                            </button>
                        </div>
                    </div>
                </div>
                <ConnectionPanel
                    v-else
                    title="创建房间"
                    subtitle="你是房主：朋友凭房间码直接连进你这边开着的游戏世界。"
                >
                    <ol class="tc-guide">
                        <li>先进游戏，把要一起玩的世界「对局域网开放」。</li>
                        <li>回到这里点「创建陶瓦房间」，房间码会出现在「当前会话」。</li>
                        <li>把 U/ 开头的房间码发给好友，等他们加入。</li>
                    </ol>
                    <label class="connection-field tc-field">
                        <span>游戏内名字</span>
                        <input
                            data-ui="TerracottaPanel:6810d6fb09db"
                            v-model="playerName"
                            class="input"
                            maxlength="16"
                            placeholder="可选，默认 WUHUICL"
                            :disabled="busy"
                        />
                    </label>
                    <div class="connection-actions">
                        <button
                            data-ui="TerracottaPanel:f0d7559a0af1"
                            class="btn btn-gold tc-main-btn"
                            :disabled="busy"
                            @click="
                                mode = 'host';
                                start();
                            "
                        >
                            {{ busy && mode === 'host' ? '处理中…' : '创建陶瓦房间' }}
                        </button>
                    </div>
                </ConnectionPanel>
            </template>

            <!-- 加入房间 -->
            <template v-else-if="tab === 'join'">
                <div v-if="opsBlockReason" class="connection-notice tc-notice" data-ui="tc:join-blocked">
                    <UiGlyph name="warning" :size="19" />
                    <div>
                        <strong>{{ !toolReady ? '先下载陶瓦工具' : '已有进行中的会话' }}</strong>
                        <p>{{ opsBlockReason }}</p>
                        <div class="connection-actions tc-notice-actions">
                            <button v-if="!toolReady" class="btn btn-gold btn-sm" @click="tab = 'tool'">
                                去下载工具
                            </button>
                            <button v-else class="btn btn-gold btn-sm" @click="tab = 'session'">
                                查看当前会话
                            </button>
                        </div>
                    </div>
                </div>
                <ConnectionPanel
                    v-else
                    title="加入房间"
                    subtitle="输入房主发给你的官方四段房间码；加入成功后，本地地址会出现在「当前会话」。"
                >
                    <label class="connection-field tc-field">
                        <span>房间码</span>
                        <input
                            data-ui="TerracottaPanel:c407f428b093"
                            v-model="roomCode"
                            class="input room-input"
                            placeholder="U/XXXX-XXXX-XXXX-XXXX"
                            :disabled="busy"
                            @keydown.enter="joinNow"
                        />
                    </label>
                    <p
                        data-ui="TerracottaPanel:9bf07a151f37"
                        v-if="roomCode.trim() && !joinCodeValid"
                        class="connection-muted"
                    >
                        格式：U/ 开头 + 四段各 4 位（例如 U/AB12-CD34-EF56-GH78）。
                    </p>
                    <div class="connection-actions">
                        <button
                            data-ui="TerracottaPanel:353f1b95db3e"
                            class="btn btn-gold tc-main-btn"
                            :disabled="busy || !joinCodeValid"
                            @click="joinNow"
                        >
                            {{ busy && mode === 'join' ? '连接中…' : '加入房间' }}
                        </button>
                    </div>
                </ConnectionPanel>
            </template>

            <!-- 当前会话 -->
            <template v-else-if="tab === 'session'">
                <ConnectionPanel
                    v-if="inSession"
                    title="当前会话"
                    subtitle="连接过程中的阶段与产出都集中在这里。"
                >
                    <template #action>
                        <ConnectionStatus
                            :tone="
                                error
                                    ? 'danger'
                                    : connected || (hosting && status?.phase === 'ready')
                                      ? 'success'
                                      : 'pending'
                            "
                            :label="
                                error
                                    ? '操作未完成'
                                    : connected
                                      ? '已连接 · 地址就绪'
                                      : hosting && status?.phase === 'ready'
                                        ? '房间已就绪'
                                        : '正在连接'
                            "
                        />
                    </template>

                    <!-- 房主：房间码就绪 -->
                    <div
                        data-ui="TerracottaPanel:206e7b16e50e"
                        v-if="hosting && status?.room"
                        class="tc-deliverable"
                        :class="{ ok: status.phase === 'ready' }"
                    >
                        <p class="tc-deliverable-label">房间码（发给好友）</p>
                        <p class="tc-deliverable-value">
                            <code>{{ status.room }}</code>
                            <button
                                data-ui="TerracottaPanel:20b2d500a6d2"
                                class="btn btn-ghost copy-mini"
                                @click="copy(status.room)"
                            >
                                复制房间码
                            </button>
                        </p>
                        <p class="connection-muted">
                            把 U/ 开头的房间码发给好友；好友用 WUHUICL 或陶瓦官方工具输入即可加入。你的游戏
                            世界保持「对局域网开放」即可。
                        </p>
                    </div>

                    <!-- 加入方：本地地址就绪 -->
                    <div
                        data-ui="TerracottaPanel:df0a2c1c5a0a"
                        v-else-if="connected"
                        class="tc-deliverable ok"
                        aria-live="polite"
                    >
                        <p class="tc-deliverable-label">本地地址（游戏内直接连接用）</p>
                        <p class="tc-deliverable-value">
                            <code>{{ status?.url }}</code>
                            <button
                                data-ui="TerracottaPanel:ce5c6200c8bf"
                                class="btn btn-ghost copy-mini"
                                @click="copy(status?.url)"
                            >
                                复制地址
                            </button>
                        </p>
                        <ol class="tc-guide">
                            <li>打开 Minecraft（与房主相同的实例与版本）</li>
                            <li>进入「多人游戏」→「直接连接」</li>
                            <li>粘贴上方地址并加入</li>
                        </ol>
                    </div>

                    <!-- 进行中：阶段说明 -->
                    <p v-else-if="busySession" class="connection-muted" aria-live="polite">
                        当前阶段：{{
                            status?.phase === 'starting'
                                ? '启动陶瓦引擎'
                                : status?.phase === 'joining'
                                  ? '正在加入房间'
                                  : status?.phase === 'hosting'
                                    ? '正在创建房间，等待房间码'
                                    : '处理中'
                        }}{{ status?.stateRaw ? ` · 引擎状态 ${status.stateRaw}` : '' }}
                    </p>

                    <!-- 阶段条 -->
                    <ol
                        data-ui="TerracottaPanel:8c761921c1c5"
                        v-if="steps.length"
                        class="tc-steps"
                        aria-label="连接过程"
                    >
                        <li
                            data-ui="TerracottaPanel:25a2ddf62ba7"
                            v-for="s in steps"
                            :key="s.label"
                            class="tc-step"
                            :class="s.state"
                        >
                            <span data-ui="TerracottaPanel:f1d7550a3d17" class="tc-step-dot" aria-hidden="true"
                                ><UiGlyph v-if="s.state === 'done'" name="check" :size="10"
                            /></span>
                            <span class="tc-step-copy"
                                ><strong>{{ s.label }}</strong
                                ><small v-if="s.detail">{{ s.detail }}</small></span
                            >
                        </li>
                    </ol>

                    <div class="connection-actions">
                        <button
                            data-ui="TerracottaPanel:e4686b6661b3"
                            class="btn btn-ghost"
                            :disabled="busy"
                            @click="stop"
                        >
                            {{ busySession ? '取消连接' : hosting && status?.phase === 'ready' ? '关闭房间' : '断开' }}
                        </button>
                    </div>
                </ConnectionPanel>
                <!-- 空态（EmptyState 收编：说明 + 双入口，UX「空状态指引」） -->
                <EmptyState
                    v-else
                    data-ui="tc:session-empty"
                    title="现在没有进行中的会话"
                    description="去创建一个房间把房号发给朋友，或者加入朋友的房间。"
                >
                    <template #action>
                        <div class="connection-actions tc-empty-actions">
                            <button class="btn btn-gold" @click="tab = 'create'">去创建房间</button>
                            <button class="btn btn-ghost" @click="tab = 'join'">去加入房间</button>
                        </div>
                    </template>
                </EmptyState>
            </template>

            <!-- 工具 -->
            <template v-else>
                <ConnectionPanel
                    v-if="!toolReady || downloading"
                    title="下载陶瓦联机工具"
                    subtitle="首次使用需要获取一次官方工具；下载完成后自动校验 SHA-256 并解压，之后无需再下载。"
                >
                    <template v-if="downloading">
                        <!-- 原生 <progress> → 全局 .progress-bar 收编（UI_SYSTEM §5 进度条唯一形态） -->
                        <div
                            data-ui="TerracottaPanel:6f7c029c3008"
                            class="progress-bar tc-progress"
                            :class="{ 'is-indeterminate': downloadPercent == null }"
                            role="progressbar"
                            aria-label="陶瓦工具下载进度"
                            :aria-valuenow="downloadPercent ?? undefined"
                            aria-valuemin="0"
                            aria-valuemax="100"
                        >
                            <div
                                class="progress-bar-fill"
                                :style="downloadPercent == null ? undefined : { width: downloadPercent + '%' }"
                            ></div>
                        </div>
                        <p class="connection-muted">
                            {{ ((status.downloaded || 0) / 1048576).toFixed(1) }} MB{{
                                status.total ? ' / ' + (status.total / 1048576).toFixed(1) + ' MB' : ''
                            }}
                            · 下载完成后自动校验并解压
                        </p>
                        <div class="connection-actions">
                            <button
                                data-ui="TerracottaPanel:1590618aaf7d"
                                class="btn btn-ghost"
                                @click="cancelInstall"
                            >
                                取消下载
                            </button>
                        </div>
                    </template>
                    <template v-else>
                        <button
                            data-ui="TerracottaPanel:bcda19ff0078"
                            class="btn btn-gold tc-download-btn"
                            :disabled="busy"
                            @click="install"
                        >
                            下载并校验工具
                        </button>
                        <p class="connection-muted">
                            工具来自陶瓦联机官方发布（GitHub burningtnt/Terracotta，基于 EasyTier，
                            AGPL-3.0），下载后会分别校验压缩包与程序的 SHA-256。
                        </p>
                    </template>
                </ConnectionPanel>
                <template v-else>
                    <ConnectionPanel
                        title="陶瓦工具"
                        :subtitle="`官方工具已安装并通过完整性校验${status?.toolVersion ? ' · ' + status.toolVersion : ''}。`"
                    >
                        <p class="mono tc-binary-path">{{ status.binaryPath }}</p>
                        <p class="connection-muted">
                            极端情况下校验失败时，删除后回到这里重新下载即可。
                        </p>
                        <div class="connection-actions">
                            <button class="btn btn-ghost" :disabled="busy" @click="refresh">
                                重新校验
                            </button>
                        </div>
                    </ConnectionPanel>

                    <details class="connection-details" data-ui="TerracottaPanel:9ff4da54ff0d">
                        <summary>陶瓦引擎日志（{{ logs.length }} 条）</summary>
                        <div data-ui="TerracottaPanel:387469b5876d" class="log-tools">
                            <button
                                data-ui="TerracottaPanel:1e6ff6dd4e33"
                                class="btn btn-ghost copy-mini"
                                :disabled="!logs.length"
                                @click="copyLogs"
                            >
                                复制日志
                            </button>
                        </div>
                        <div
                            data-ui="TerracottaPanel:9ba0004a10df"
                            ref="logViewport"
                            class="connection-log-viewport"
                            aria-live="polite"
                            tabindex="0"
                        >
                            <p
                                data-ui="TerracottaPanel:8607ff626ce4"
                                v-for="(l, i) in logs"
                                :key="i"
                                class="connection-log-line"
                            >
                                <span data-ui="TerracottaPanel:41e49f406b4c" class="log-ts">{{ l.ts }}</span
                                >{{ l.text }}
                            </p>
                            <p data-ui="TerracottaPanel:c7a082d9c420" v-if="!logs.length" class="connection-muted">
                                暂无日志。
                            </p>
                        </div>
                    </details>

                    <details class="connection-details" data-ui="TerracottaPanel:3fe99c5bebdf">
                        <summary>关于陶瓦联机</summary>
                        <div data-ui="TerracottaPanel:47f49123c2eb" class="connection-detail-content">
                            <p>陶瓦联机（Terracotta）是 burningtnt 维护的独立开源联机项目，基于 EasyTier，AGPL-3.0 协议。</p>
                            <p>房间码为官方四段格式 U/XXXX-XXXX-XXXX-XXXX，与陶瓦官方工具互通；连接效果取决于双方网络。</p>
                        </div>
                    </details>
                </template>
            </template>
        </div>
    </div>
</template>
<style scoped>
.tc-console {
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-width: 0;
}
.tc-tabbody {
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-width: 0;
    animation: tc-in 0.18s var(--ease-out) both;
}
@keyframes tc-in {
    from {
        opacity: 0;
        transform: translateY(6px);
    }
    to {
        opacity: 1;
        transform: none;
    }
}
.tc-progress {
    width: 100%;
    margin: 4px 0;
}
.tc-download-btn {
    align-self: flex-start;
    min-height: 42px;
    font-weight: 700;
}
.tc-notice {
    align-items: flex-start;
}
.tc-notice-actions {
    margin-top: 10px;
}
.tc-field {
    max-width: 460px;
}

/* 步骤指引（创建房间的三步） */
.tc-guide {
    margin: 0;
    padding-left: 20px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
    line-height: 1.8;
    color: var(--text-dim);
}

/* 会话卡产出块：房间码 / 本地地址 */
.tc-deliverable {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--card-2);
}
.tc-deliverable.ok {
    border-color: color-mix(in srgb, var(--ok) 40%, var(--border));
}
.tc-deliverable-label {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-dim);
}
.tc-deliverable-value {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 12px;
    margin: 2px 0;
    min-height: 32px;
}
.tc-deliverable-value code {
    font-size: 18px;
    font-weight: 700;
    letter-spacing: 0.05em;
    overflow-wrap: anywhere;
}
.room-input {
    text-transform: uppercase;
    letter-spacing: 0.1em;
}
.copy-mini {
    padding: 4px 12px;
    font-size: 12px;
    min-height: 28px;
}
.log-ts {
    color: color-mix(in srgb, var(--text-dim) 70%, transparent);
    margin-right: 8px;
}
.log-tools {
    display: flex;
    justify-content: flex-end;
    margin-top: 10px;
}
.tc-binary-path {
    font-size: 12px;
    overflow-wrap: anywhere;
    margin: 0;
}

/* 阶段条 */
.tc-steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
}
.tc-step {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--card-2);
}
.tc-step.active {
    border-color: color-mix(in srgb, var(--accent-2) 55%, var(--border));
}
.tc-step.done {
    border-color: color-mix(in srgb, var(--ok) 40%, var(--border));
}
.tc-step-dot {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--card);
    border: 1px solid var(--border-strong);
    flex: none;
}
.tc-step.done .tc-step-dot {
    background: var(--ok-soft);
    color: var(--ok);
    border-color: transparent;
}
.tc-step.active .tc-step-dot {
    background: var(--accent-soft);
    color: var(--accent-2);
    border-color: transparent;
}
.tc-step-copy {
    display: flex;
    flex-direction: column;
    min-width: 0;
}
.tc-step-copy strong {
    font-size: 12px;
    font-weight: 600;
}
.tc-step-copy small {
    color: var(--text-dim);
    font-size: 12px;
}

/* 主按钮 */
.tc-main-btn {
    min-height: 40px;
    font-weight: 700;
}

.tc-empty-actions {
    justify-content: center;
}
</style>
