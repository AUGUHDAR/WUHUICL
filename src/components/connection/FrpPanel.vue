<script setup lang="ts">
import ReferenceLinks from './ReferenceLinks.vue';
import UiGlyph from '../UiGlyph.vue';
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import ConnectionPanel from './ConnectionPanel.vue';
import ConfirmModal from '../ConfirmModal.vue';
import ConnectionStatus from './ConnectionStatus.vue';
import { copyText } from '../../api';
import { toast } from '../../store';

/** 与后端 frp.rs 的快照/节点字段对齐（类型仅前端展示用） */
interface ManagedTunnel {
    id: string;
    name: string;
    nodeName: string;
    localIp: string;
    status: string;
    message: string;
    remoteAddress: string | null;
    desired: boolean;
    busy: boolean;
    deleting?: boolean;
    logs: Array<{ stream: string; text: string }>;
    config: { tunnelId?: string; localPort?: number } | null;
}
interface FrpNode {
    id: number;
    name: string;
    host: string;
    description: string;
    free: boolean;
    online: boolean;
    load: number | null;
    udp: boolean;
    mainland: boolean;
    canCreate: boolean;
    beta: boolean;
}
interface FrpNodesResult {
    nodes: FrpNode[];
    tunnels: unknown[];
}

/** 分区状态跨页面切换保留（与 VoxLink 的 section 同模式） */
type FrpTab = 'tunnels' | 'create' | 'nodes' | 'account';
const tab = ref<FrpTab>('tunnels');
const NAV: Array<{ key: FrpTab; label: string }> = [
    { key: 'tunnels', label: '我的隧道' },
    { key: 'create', label: '新建隧道' },
    { key: 'nodes', label: '节点参考' },
    { key: 'account', label: '账号密钥' }
];

const kamucl = window.kamucl;
const tunnels = ref<ManagedTunnel[]>([]);
const form = reactive({ accessKey: '' });
const operations = reactive(new Set<string>());
const stopping = reactive(new Set<string>());
const errorMsg = ref('');
const accountReady = computed(() => !!nodesResult.value && !nodesError.value);
const deleteTarget = ref<ManagedTunnel | null>(null);
const deleting = ref(false);
const deleteError = ref('');
function askDelete(t: ManagedTunnel) {
    if (t.deleting) return;
    deleteError.value = '';
    deleteTarget.value = t;
}
function cancelDelete() {
    if (!deleting.value) deleteTarget.value = null;
}
async function confirmDelete() {
    const target = deleteTarget.value;
    if (!target || deleting.value) return;
    deleting.value = true;
    deleteError.value = '';
    try {
        const result = (await kamucl.invoke('frp:delete-tunnel', {
            id: target.id,
            confirmed: true
        })) as { remoteDisconnectPending: boolean };
        tunnels.value = tunnels.value.filter((t) => t.id !== target.id);
        nodesResult.value = null;
        deleteTarget.value = null;
        toast(
            result.remoteDisconnectPending
                ? '远端隧道已删除，本地连接已停止；其他设备连接可能尚未断开'
                : `已从樱花穿透删除「${target.name}」`,
            result.remoteDisconnectPending ? 'info' : 'success'
        );
    } catch (e) {
        deleteError.value = errText(e);
        toast(deleteError.value, 'error');
    } finally {
        deleting.value = false;
        await refreshStatus();
    }
}
const nodesResult = ref<FrpNodesResult | null>(null);
const nodesLoading = ref(false);
const nodesError = ref('');
const onlyFree = ref(true);
const creation = reactive({ name: 'Minecraft', node: '', localPort: '', remotePort: '' });
const creating = ref(false);
const creatableNodes = computed(() =>
    (nodesResult.value?.nodes || []).filter(
        (n) => (onlyFree.value ? n.free : true) && n.online && n.canCreate
    )
);
const connected = computed(() => tunnels.value.filter((t) => t.status === 'running').length);
const busy = computed(() => creating.value || nodesLoading.value);
let disposed = false;
const statusLabel: Record<string, string> = {
    idle: '未启动',
    starting: '正在连接',
    running: '已连接',
    auth_failed: '认证失败',
    tunnel_offline: '隧道不可用',
    error: '连接失败',
    stopped: '已停止'
};
const active = (t: ManagedTunnel) => t.status === 'running' || t.status === 'starting';
const tone = (t: ManagedTunnel): 'neutral' | 'success' | 'danger' | 'pending' =>
    t.busy
        ? 'pending'
        : t.status === 'running'
          ? 'success'
          : t.status === 'starting'
            ? 'pending'
            : ['auth_failed', 'tunnel_offline', 'error'].includes(t.status)
              ? 'danger'
              : 'neutral';
const errText = (e: unknown) =>
    (e instanceof Error ? e.message : String(e)).replace(
        /^Error invoking remote method '[^']*': (Error: )?/,
        ''
    );
watch(
    () => form.accessKey,
    () => {
        nodesResult.value = null;
    }
);
async function refreshStatus() {
    try {
        const result = (await kamucl.invoke('frp:status')) as {
            accessKey: string;
            tunnels: ManagedTunnel[];
        };
        if (disposed) return;
        tunnels.value = result.tunnels || [];
        if (!form.accessKey && result.accessKey) form.accessKey = result.accessKey;
    } catch (e) {
        errorMsg.value = errText(e);
    }
}
async function control(t: ManagedTunnel, stop: boolean) {
    if (t.deleting || (stop ? stopping.has(t.id) : operations.has(t.id))) return;
    if (stop) stopping.add(t.id);
    operations.add(t.id);
    errorMsg.value = '';
    try {
        await kamucl.invoke(stop ? 'frp:stop' : 'frp:start', { id: t.id });
        if (stop) toast(`已停止「${t.name}」，下次启动不会自动恢复`, 'success');
    } catch (e) {
        errorMsg.value = errText(e);
        toast(errorMsg.value, 'error');
    } finally {
        operations.delete(t.id);
        stopping.delete(t.id);
        await refreshStatus();
    }
}
async function loadNodes(refresh = false, notify = true): Promise<boolean> {
    if (nodesLoading.value) return false;
    nodesLoading.value = true;
    nodesError.value = '';
    try {
        const result = (await kamucl.invoke('frp:nodes', {
            accessKey: form.accessKey.trim(),
            refresh
        })) as FrpNodesResult;
        if (!Array.isArray(result?.nodes)) throw new Error('节点列表查询失败，请重试');
        if (!Array.isArray(result.tunnels))
            throw new Error('隧道列表查询失败，请检查密钥权限后重试');
        if (disposed) return false;
        nodesResult.value = result;
        await refreshStatus();
        if (tab.value === 'account') tab.value = 'tunnels';
        if (notify)
            toast(
                `已读取成功：${result.tunnels.length} 条隧道，${result.nodes.length} 个节点`,
                'success'
            );
        return true;
    } catch (e) {
        if (!disposed) {
            nodesError.value = errText(e);
            if (notify) toast(`读取失败：${nodesError.value}`, 'error');
        }
        return false;
    } finally {
        nodesLoading.value = false;
    }
}
async function createTunnel() {
    if (creating.value) return;
    creating.value = true;
    errorMsg.value = '';
    try {
        await kamucl.invoke('frp:create-tunnel', {
            accessKey: form.accessKey.trim(),
            tunnel: {
                name: creation.name,
                node: Number(creation.node),
                localPort: Number(creation.localPort),
                remotePort: Number(creation.remotePort) || undefined
            }
        });
        const loaded = await loadNodes(true, false);
        tab.value = 'tunnels';
        toast(
            loaded ? '隧道已创建，在卡片中启动即可' : '隧道已创建，请刷新列表后查看',
            loaded ? 'success' : 'info'
        );
        creation.localPort = '';
        creation.remotePort = '';
    } catch (e) {
        errorMsg.value = errText(e);
        toast(errorMsg.value, 'error');
    } finally {
        creating.value = false;
    }
}
async function copyWebsite() {
    toast((await copyText('https://www.natfrp.com/')) ? '已复制樱花穿透网址' : '复制失败', 'info');
}
async function copyRemote(address: string) {
    toast((await copyText(address)) ? '已复制远程地址' : '复制失败', 'info');
}
let unsubscribe: (() => void) | undefined;
/** 首次进入时密钥已保存：静默补一次读取，避免重启后还要手动点 */
onMounted(async () => {
    unsubscribe = kamucl.on('frp:event', (raw: unknown) => {
        const t = (raw as { tunnel?: ManagedTunnel })?.tunnel;
        if (!t || disposed) return;
        const index = tunnels.value.findIndex((x) => x.id === t.id);
        if (index >= 0) tunnels.value[index] = t;
        else tunnels.value.push(t);
    });
    await refreshStatus();
    if (!disposed && form.accessKey.trim() && !nodesResult.value) void loadNodes(false, false);
});
onBeforeUnmount(() => {
    disposed = true;
    unsubscribe?.();
});
</script>

<template>
    <div data-ui="FrpPanel:ab59a69f21ab" class="frp-console">
        <ConfirmModal
            :open="!!deleteTarget"
            title="删除樱花隧道"
            :message="
                deleteTarget
                    ? `确认删除「${deleteTarget.name}」（#${deleteTarget.config?.tunnelId}）？这会从樱花穿透账号中永久删除该隧道，停止本地连接并取消自动恢复，无法撤销。其他隧道不受影响。${deleteError ? '\n' + deleteError : ''}`
                    : ''
            "
            confirm-text="从樱花穿透删除"
            :busy="deleting"
            @confirm="confirmDelete"
            @cancel="cancelDelete"
        />

        <!-- 分区导航（VoxLink 同款胶囊） -->
        <nav class="connect-tabs" aria-label="FRP 分区" data-ui="frp:nav">
            <button
                v-for="item in NAV"
                :key="item.key"
                class="connect-tab"
                :class="{ active: tab === item.key }"
                :data-ui="'frp:tab:' + item.key"
                @click="tab = item.key"
            >
                {{ item.label }}<span v-if="item.key === 'tunnels' && connected" class="dot"></span>
            </button>
        </nav>

        <p data-ui="FrpPanel:54b1d9fe0e6e" v-if="errorMsg" class="connection-error" role="alert">
            {{ errorMsg }}
        </p>

        <div class="frp-tabbody" :key="tab">
            <!-- 我的隧道 -->
            <template v-if="tab === 'tunnels'">
                <div v-if="accountReady" class="frp-account-bar" data-ui="FrpPanel:57467d5ded4a">
                    <span class="frp-account-text">
                        樱花穿透账号已连接 · {{ tunnels.length }} 条隧道 · 密钥保存在本机
                    </span>
                    <span class="frp-account-actions">
                        <button
                            class="btn btn-ghost btn-sm"
                            :disabled="nodesLoading"
                            @click="loadNodes(true)"
                        >
                            {{ nodesLoading ? '正在读取…' : '重新读取' }}
                        </button>
                        <button class="btn btn-ghost btn-sm" @click="tab = 'account'">
                            编辑密钥
                        </button>
                    </span>
                </div>

                <div v-if="tunnels.length" class="frp-tunnel-grid" aria-label="隧道控制区域" data-ui="frp:tunnels">
                    <article
                        v-for="t in tunnels"
                        :key="t.id"
                        class="frp-card"
                        :class="{ 'is-running': t.status === 'running' }"
                        :data-ui="'frp:tunnel:' + t.id"
                    >
                        <header data-ui="FrpPanel:c5911bc51b7c" class="frp-card-head">
                            <div class="frp-card-title">
                                <h3>{{ t.name }}</h3>
                                <p>{{ t.nodeName || '节点信息待读取' }} · #{{ t.config?.tunnelId }}</p>
                            </div>
                            <ConnectionStatus
                                :tone="tone(t)"
                                :label="
                                    t.deleting ? '正在删除' : t.busy ? '正在准备' : statusLabel[t.status]
                                "
                            />
                        </header>

                        <div data-ui="FrpPanel:e157d6943eab" class="frp-endpoints">
                            <div class="frp-endpoint">
                                <span>本地服务</span>
                                <strong>{{ t.localIp }}:{{ t.config?.localPort || '未配置' }}</strong>
                            </div>
                            <div class="frp-endpoint frp-endpoint-remote">
                                <span>远程地址（发给好友，游戏内直接连接用）</span>
                                <span class="frp-endpoint-value">
                                    <strong>{{
                                        t.remoteAddress ||
                                        (t.status === 'running' ? '等待服务返回地址…' : '启动后显示')
                                    }}</strong>
                                    <button
                                        v-if="t.remoteAddress"
                                        class="btn btn-ghost btn-sm"
                                        @click="copyRemote(t.remoteAddress)"
                                    >
                                        复制地址
                                    </button>
                                </span>
                            </div>
                        </div>

                        <p
                            data-ui="FrpPanel:95340b6c69f4"
                            v-if="t.busy || tone(t) === 'danger'"
                            class="frp-card-message"
                            :class="{ danger: tone(t) === 'danger' }"
                        >
                            {{ t.busy ? '正在检查隧道并准备连接…' : t.message }}
                        </p>

                        <footer data-ui="FrpPanel:10ce12df3ad2" class="frp-card-foot">
                            <small>{{ t.desired ? '启动器重启后自动恢复' : '已停止自动恢复' }}</small>
                            <div data-ui="FrpPanel:3acd6476d7b7" class="frp-card-actions">
                                <button
                                    data-ui="FrpPanel:d8c4790e0c8e"
                                    v-if="!active(t) && !t.busy"
                                    class="btn btn-gold"
                                    :disabled="t.deleting || operations.has(t.id)"
                                    @click="control(t, false)"
                                >
                                    {{ tone(t) === 'danger' ? '重试连接' : '启动隧道' }}</button
                                ><button
                                    data-ui="FrpPanel:1cbfb09e7f09"
                                    v-if="active(t) || t.desired || t.busy"
                                    class="btn btn-ghost"
                                    :disabled="t.deleting || stopping.has(t.id)"
                                    @click="control(t, true)"
                                >
                                    {{ t.busy ? '取消启动' : '停止隧道' }}
                                </button>
                                <details
                                    class="frp-more"
                                    @keydown.esc="($event.currentTarget as HTMLDetailsElement).open = false"
                                >
                                    <summary class="btn btn-ghost btn-sm" :aria-label="'更多操作 ' + t.name">
                                        ⋯
                                    </summary>
                                    <div>
                                        <button
                                            data-ui="FrpPanel:05be50a5135c"
                                            class="btn btn-danger btn-sm"
                                            :disabled="t.deleting"
                                            @click="askDelete(t)"
                                        >
                                            删除隧道
                                        </button>
                                    </div>
                                </details>
                            </div>
                        </footer>

                        <details data-ui="FrpPanel:817cbf8159bb" class="frp-card-logs">
                            <summary>运行日志 · {{ t.logs.length }} 条</summary>
                            <div data-ui="FrpPanel:3156be53d1a0" class="connection-log-viewport">
                                <p data-ui="FrpPanel:6f7846e870fd" v-if="!t.logs.length" class="connection-muted">
                                    尚无日志
                                </p>
                                <p
                                    data-ui="FrpPanel:ff75ffc93400"
                                    v-for="(entry, i) in t.logs"
                                    :key="i"
                                    class="mono connection-log-line"
                                >
                                    [{{ entry.stream }}] {{ entry.text }}
                                </p>
                            </div>
                        </details>
                    </article>
                </div>

                <div v-else class="frp-empty">
                    <template v-if="!accountReady">
                        <h3>还没有连接樱花穿透账号</h3>
                        <p>先在「账号密钥」里填入访问密钥并读取账号，隧道会显示在这里。</p>
                        <button class="btn btn-gold" @click="tab = 'account'">去填密钥</button>
                    </template>
                    <template v-else>
                        <h3>还没有隧道</h3>
                        <p>创建一条隧道并启动后，这里会出现远程地址，把它发给好友就能一起玩。</p>
                        <button class="btn btn-gold" @click="tab = 'create'">新建隧道</button>
                    </template>
                </div>
            </template>

            <!-- 新建隧道 -->
            <template v-else-if="tab === 'create'">
                <div v-if="!accountReady" class="connection-notice frp-notice" data-ui="frp:create-blocked">
                    <UiGlyph name="warning" :size="19" />
                    <div>
                        <strong>先连接樱花穿透账号</strong>
                        <p>创建隧道需要先在「账号密钥」里填写访问密钥并读取账号，读取成功后回来选择节点。</p>
                        <div class="connection-actions frp-notice-actions">
                            <button class="btn btn-gold btn-sm" @click="tab = 'account'">去填密钥</button>
                        </div>
                    </div>
                </div>
                <ConnectionPanel
                    v-else
                    title="创建新隧道"
                    subtitle="选择服务节点，再填入游戏对局域网开放后显示的端口。创建成功后不会自动运行，需要到「我的隧道」手动启动。"
                >
                    <div data-ui="FrpPanel:fc691b543124" class="frp-create-grid">
                        <label class="connection-field">
                            <span>隧道名称</span>
                            <input
                                data-ui="FrpPanel:b7caefdb0a45"
                                v-model="creation.name"
                                class="input"
                                maxlength="64"
                                placeholder="例如：好友生存世界"
                            />
                        </label>
                        <label class="connection-field">
                            <span>本地端口</span>
                            <input
                                data-ui="FrpPanel:9ca5880a3b61"
                                v-model="creation.localPort"
                                class="input"
                                type="number"
                                min="1"
                                max="65535"
                                placeholder="游戏对局域网开放后显示的端口"
                            />
                        </label>
                        <label class="connection-field">
                            <span>服务节点</span>
                            <select data-ui="FrpPanel:e06c3d520528" v-model="creation.node" class="input">
                                <option value="">
                                    {{ nodesResult ? '选择可用节点' : '请先读取账号节点' }}
                                </option>
                                <option v-for="n in creatableNodes" :key="n.id" :value="String(n.id)">
                                    {{ n.name }} · {{ n.free ? '免费' : '专业版'
                                    }}{{ n.load !== null ? ' · 负载 ' + n.load + '%' : '' }}
                                </option>
                            </select>
                        </label>
                        <label class="connection-field">
                            <span>远程端口（可选）</span>
                            <input
                                data-ui="FrpPanel:0ce18bb53250"
                                v-model="creation.remotePort"
                                class="input"
                                type="number"
                                min="1"
                                max="65535"
                                placeholder="留空由樱花穿透分配"
                            />
                        </label>
                    </div>
                    <label data-ui="FrpPanel:c99a7ef6ce06" class="frp-free-row">
                        <input v-model="onlyFree" type="checkbox" />
                        <span>只看免费节点<small>专业版节点需要 natfrp 专业版账号</small></span>
                    </label>
                    <div class="frp-create-actions">
                        <p v-if="!creatableNodes.length && nodesResult" class="connection-muted">
                            当前筛选下没有可用的节点，可以关闭「只看免费节点」或在「节点参考」里看看情况。
                        </p>
                        <button
                            data-ui="FrpPanel:1769901eab4f"
                            class="btn btn-gold"
                            :disabled="
                                creating || !form.accessKey.trim() || !creation.node || !creation.localPort
                            "
                            @click="createTunnel"
                        >
                            {{ creating ? '正在创建…' : '创建 TCP 隧道' }}
                        </button>
                    </div>
                </ConnectionPanel>
            </template>

            <!-- 节点参考 -->
            <template v-else-if="tab === 'nodes'">
                <ConnectionPanel
                    v-if="nodesResult"
                    title="节点参考"
                    subtitle="在「新建隧道」中选择节点；负载和说明供选择前参考，专业版节点需要 natfrp 专业版账号权限。"
                >
                    <ul data-ui="FrpPanel:d02fe3adf345" class="node-list" aria-label="节点列表">
                        <li data-ui="FrpPanel:9c24368300dc" v-for="n in nodesResult.nodes" :key="n.id" class="node-item">
                            <span
                                data-ui="FrpPanel:2a396bfd1af7"
                                class="node-online"
                                :class="{ on: n.online }"
                                role="img"
                                :aria-label="n.online ? '在线' : '离线'"
                            ></span>
                            <span data-ui="FrpPanel:0b012af541ee" class="node-main">
                                <span data-ui="FrpPanel:6cdd73008975" class="node-title">
                                    <strong>{{ n.name }}</strong>
                                    <em data-ui="FrpPanel:bbbe3ac23456" class="node-badge" :class="n.free ? 'free' : 'vip'">{{
                                        n.free ? '免费' : '专业版'
                                    }}</em>
                                    <em data-ui="FrpPanel:e88759215898" v-if="n.mainland" class="node-badge">内地</em>
                                    <em data-ui="FrpPanel:95bd84a9e9a0" v-if="n.udp" class="node-badge">UDP</em>
                                    <em data-ui="FrpPanel:d6ea22acee8d" v-if="!n.canCreate" class="node-badge warn">满载</em>
                                    <em data-ui="FrpPanel:218b0217dc78" v-if="n.beta" class="node-badge">BETA</em>
                                </span>
                                <small data-ui="FrpPanel:be7c9e5f6c63" v-if="n.description" class="connection-muted node-desc">{{
                                    n.description
                                }}</small>
                                <small data-ui="FrpPanel:6fce33a59021" class="mono node-host">{{ n.host }}</small>
                            </span>
                            <span data-ui="FrpPanel:a0710cc1b6d3" class="node-load">
                                <small data-ui="FrpPanel:601b8e03c020" class="connection-muted">负载</small>
                                <strong>{{ n.load === null ? '—' : n.load + '%' }}</strong>
                            </span>
                        </li>
                    </ul>
                </ConnectionPanel>
                <div v-else class="connection-notice frp-notice" data-ui="frp:nodes-blocked">
                    <UiGlyph name="warning" :size="19" />
                    <div>
                        <strong>{{ nodesLoading ? '正在读取节点…' : '还没有读取节点' }}</strong>
                        <p>节点列表跟账号走：先在「账号密钥」里填写访问密钥并读取账号。</p>
                        <div class="connection-actions frp-notice-actions">
                            <button class="btn btn-gold btn-sm" :disabled="nodesLoading" @click="tab = 'account'">
                                去填密钥
                            </button>
                        </div>
                    </div>
                </div>
            </template>

            <!-- 账号密钥 -->
            <template v-else>
                <ConnectionPanel
                    title="连接樱花穿透账号"
                    subtitle="填写访问密钥读取账号下的隧道与节点；密钥仅保存在本机，不会上传。"
                >
                    <ol class="frp-guide">
                        <li>
                            注册或登录樱花穿透（<span class="mono">natfrp.com</span>），在用户信息页复制
                            <strong>访问密钥</strong>。
                        </li>
                        <li>粘贴到下方输入框，点击「读取我的隧道与节点」。</li>
                        <li>之后创建或启动隧道，把远程地址发给好友即可联机。</li>
                    </ol>
                    <div data-ui="FrpPanel:08816fe7bdc1" class="frp-account-row">
                        <label class="connection-field">
                            <span>访问密钥</span>
                            <input
                                data-ui="FrpPanel:a4d20e81e801"
                                v-model="form.accessKey"
                                class="input"
                                type="password"
                                autocomplete="off"
                                placeholder="粘贴用户信息页的访问密钥"
                                :disabled="busy"
                            />
                        </label>
                        <button
                            data-ui="FrpPanel:595aea894985"
                            class="btn btn-gold"
                            :disabled="busy || !form.accessKey.trim()"
                            @click="loadNodes(true)"
                        >
                            {{ nodesLoading ? '正在读取…' : '读取我的隧道与节点' }}
                        </button>
                    </div>
                    <div data-ui="FrpPanel:b0c9c097bfa7" class="frp-account-help">
                        <ReferenceLinks
                            :links="[
                                { label: '樱花穿透官网', url: 'https://www.natfrp.com/' },
                                { label: '使用指南', url: 'https://doc.natfrp.com/' }
                            ]"
                        >
                            <button data-ui="FrpPanel:a705e51a826a" type="button" @click="copyWebsite">
                                复制官网地址
                            </button>
                        </ReferenceLinks>
                        <small>访问密钥仅保存在本机</small>
                    </div>
                    <p v-if="nodesError" class="connection-error" role="alert">{{ nodesError }}</p>
                </ConnectionPanel>
            </template>
        </div>
    </div>
</template>

<style scoped>
.frp-console {
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-width: 0;
}
.frp-tabbody {
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-width: 0;
    animation: frp-in 0.18s var(--ease-out) both;
}
@keyframes frp-in {
    from {
        opacity: 0;
        transform: translateY(6px);
    }
    to {
        opacity: 1;
        transform: none;
    }
}

/* 引导卡：三步说明 + 密钥输入 */
.frp-guide {
    margin: 0;
    padding-left: 20px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
    line-height: 1.7;
    color: var(--text-dim);
}
.frp-guide strong {
    color: var(--text);
}
.frp-account-row {
    display: flex;
    align-items: flex-end;
    gap: 12px;
    flex-wrap: wrap;
}
.frp-account-row .connection-field {
    flex: 1;
    min-width: 220px;
}
.frp-account-help {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 12px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
}
.frp-account-help small {
    color: var(--text-dim);
    font-size: 12px;
}

/* 提示条（前置条件未满足时的指引） */
.frp-notice {
    align-items: flex-start;
}
.frp-notice-actions {
    margin-top: 10px;
}

/* 账号状态条 */
.frp-account-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    padding: 10px 16px;
    background: var(--surface-content);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    font-size: 13px;
}
.frp-account-text {
    flex: 1;
    min-width: 200px;
    color: var(--text-dim);
}
.frp-account-actions {
    display: flex;
    gap: 8px;
}

/* 隧道卡片：桌面双列，窄屏单列 */
.frp-tunnel-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(430px, 1fr));
    gap: 14px;
}
.frp-card {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 18px 20px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-content);
}
.frp-card.is-running {
    border-color: color-mix(in srgb, var(--ok) 45%, var(--border));
}
.frp-card-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
}
.frp-card-title {
    min-width: 0;
}
.frp-card-title h3 {
    margin: 0 0 4px;
    font-size: 16px;
    overflow-wrap: anywhere;
}
.frp-card-title p {
    margin: 0;
    color: var(--text-dim);
    font-size: 12px;
}
.frp-endpoints {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.4fr);
    gap: 10px;
}
.frp-endpoint {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    padding: 10px 12px;
    border-radius: var(--radius-md);
    background: var(--card-2);
}
.frp-endpoint > span:first-child {
    font-size: 12px;
    color: var(--text-dim);
}
.frp-endpoint strong {
    font-size: 14px;
    overflow-wrap: anywhere;
    user-select: text;
}
.frp-endpoint-value {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
}
.frp-endpoint-value strong {
    flex: 1;
    min-width: 0;
}
.frp-card-message {
    margin: 0;
    font-size: 13px;
    line-height: 1.6;
    color: var(--text-dim);
    overflow-wrap: anywhere;
}
.frp-card-message.danger {
    color: var(--danger);
}
.frp-card-foot {
    margin-top: auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
}
.frp-card-foot small {
    color: var(--text-dim);
    font-size: 12px;
}
.frp-card-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    align-items: center;
}
.frp-card-logs {
    padding-top: 10px;
    border-top: 1px solid var(--border);
}
.frp-card-logs summary {
    cursor: pointer;
    color: var(--text-dim);
    font-size: 12px;
    padding: 4px 0;
}
.frp-more {
    position: relative;
}
.frp-more summary {
    list-style: none;
}
.frp-more > div {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    padding: 8px;
    background: var(--surface-solid);
    border: 1px solid var(--border);
    z-index: 5;
    border-radius: var(--radius-md);
}

/* 空状态 */
.frp-empty {
    border: 1px dashed var(--border-strong);
    padding: 30px 20px;
    text-align: center;
    border-radius: var(--radius-md);
    color: var(--text-dim);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    background: var(--card-2);
}
.frp-empty h3 {
    margin: 0;
    color: var(--text);
    font-size: 15px;
}
.frp-empty p {
    margin: 0;
    max-width: 440px;
    line-height: 1.7;
    font-size: 13px;
}

/* 新建表单 */
.frp-create-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
}
.frp-free-row {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
}
.frp-free-row span {
    display: flex;
    flex-direction: column;
    font-size: 13px;
}
.frp-free-row small {
    color: var(--text-dim);
    font-size: 12px;
}
.frp-create-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
}
.frp-create-actions .connection-muted {
    margin: 0;
}

/* 节点参考 */
.node-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
}
.node-item {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 10px 14px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background: var(--card-2);
}
.node-online {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    flex: none;
    margin-top: 6px;
    background: var(--text-dim);
}
.node-online.on {
    background: var(--ok);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 22%, transparent);
}
.node-main {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    flex: 1;
}
.node-title {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    min-width: 0;
}
.node-title strong {
    font-size: 13px;
    overflow-wrap: anywhere;
}
.node-badge {
    display: inline-flex;
    align-items: center;
    font-style: normal;
    font-size: 12px;
    line-height: 1;
    padding: 3px 8px;
    border-radius: 999px;
    border: 1px solid var(--border-strong);
    color: var(--text-dim);
    white-space: nowrap;
}
.node-badge.free {
    color: var(--ok);
    border-color: color-mix(in srgb, var(--ok) 45%, transparent);
}
.node-badge.vip {
    color: var(--accent-2);
    border-color: color-mix(in srgb, var(--accent-2) 45%, transparent);
}
.node-badge.warn {
    color: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 45%, transparent);
}
.node-desc {
    line-height: 1.6;
}
.node-host {
    color: var(--text-dim);
    overflow-wrap: anywhere;
}
.node-load {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    flex: none;
    min-width: 52px;
}

@media (max-width: 720px) {
    .frp-tunnel-grid,
    .frp-create-grid,
    .frp-endpoints {
        grid-template-columns: 1fr;
    }
}
</style>
