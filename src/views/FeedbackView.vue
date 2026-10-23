<script setup lang="ts">
// FeedbackView.vue — 「反馈与建议」：公开板 + 匿名私信 + 提交。
// 数据走 feedbackApi（Rust feedback.rs → wuhuicl.voxdev.top）。
// 结构对齐现有视图：page-head / card / tag / btn / empty / modal-mask，颜色只用主题令牌。
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { toast } from '../store';
import { managedImageUrl } from '../managedAssets';
import SelectMenu from '../components/SelectMenu.vue';
import ConfirmModal from '../components/ConfirmModal.vue';
import UiGlyph from '../components/UiGlyph.vue';
import {
    fbAttachment,
    fbPickFiles,
    fbIssueComment,
    fbIssueDelete,
    fbIssueGet,
    fbIssueRetract,
    fbIssuesList,
    fbLogin,
    fbLogout,
    fbMeta,
    fbTicketDelete,
    fbTicketDetail,
    fbTicketReply,
    fbTicketRetract,
    fbTicketSubmit,
    fbTicketViewed,
    fbTicketsList,
    fbTicketsPoll,
    fbIssueSubmit,
    fbLogsPreview,
    type FBComment,
    type FBIssueDetail,
    type FBIssueRow,
    type FBTicketDetail,
    type FBTicketLocal
} from '../feedbackApi';

type Tab = 'board' | 'tickets';
const tab = ref<Tab>('board');

// ---------------- 元信息 / 登录 ----------------
const meta = ref<Awaited<ReturnType<typeof fbMeta>> | null>(null);
const loggedIn = computed(() => !!meta.value?.loggedIn);
const loginBusy = ref(false);
/** 配对码：网页配对页要求用户核对，必须真的显示出来 */
const pairCode = ref('');

async function refreshMeta() {
    try {
        meta.value = await fbMeta();
    } catch (e) {
        toast(`反馈服务暂不可达：${e}`, 'error');
    }
}
async function doLogin() {
    if (loginBusy.value) return;
    loginBusy.value = true;
    pairCode.value = '';
    try {
        const r = await fbLogin();
        pairCode.value = r.code;
        toast('已在浏览器打开授权页，核对配对码后完成登录', 'info');
    } catch (e) {
        loginBusy.value = false;
        pairCode.value = '';
        toast(`登录失败：${e}`, 'error');
    }
}
/** 放弃本次登录：中止后台轮询，立刻回到可操作状态。 */
async function cancelLogin() {
    try {
        await fbLoginCancel();
    } catch {
        /* 取消失败也让前端恢复可操作 */
    }
    loginBusy.value = false;
    pairCode.value = '';
    toast('已取消登录', 'info');
}
async function doLogout() {
    await fbLogout();
    await refreshMeta();
    toast('已退出登录', 'info');
    loadList();
}

// ---------------- 公开板 ----------------
const issues = ref<FBIssueRow[]>([]);
const total = ref(0);
const page = ref(1);
/** 页大小随请求一起下发，并回读服务端实际采用值——两处口径必须同源，否则页数会算错。 */
const SIZE = 20;
const pageSize = ref(SIZE);
const totalPages = computed(() => Math.max(1, Math.ceil(total.value / pageSize.value)));
const filterKind = ref('');
const filterStatus = ref('');
const keyword = ref('');
const mineOnly = ref(false);
const listLoading = ref(false);
const detail = ref<FBIssueDetail | null>(null);

const kindOptions = computed(() => [
    { value: '', label: '全部类型' },
    ...(meta.value?.kinds || []).map((k) => ({ value: k.key, label: k.name }))
]);
const statusOptions = computed(() => [
    { value: '', label: '全部状态' },
    ...(meta.value?.statuses || []).map((s) => ({ value: s.key, label: s.name }))
]);

async function loadList() {
    listLoading.value = true;
    try {
        const r = await fbIssuesList({
            kind: filterKind.value,
            status: filterStatus.value,
            q: keyword.value.trim(),
            page: page.value,
            size: SIZE,
            mine: mineOnly.value
        });
        issues.value = r.issues || [];
        total.value = r.total || 0;
        if (r.size) pageSize.value = r.size;
    } catch (e) {
        toast(`加载失败：${e}`, 'error');
    } finally {
        listLoading.value = false;
    }
}
function refilter() {
    page.value = 1;
    loadList();
}
function turnPage(d: number) {
    page.value = Math.max(1, page.value + d);
    loadList();
}

async function openIssue(num: number) {
    detail.value = null;
    try {
        detail.value = await fbIssueGet(num);
    } catch (e) {
        toast(`打开失败：${e}`, 'error');
    }
}
function closeIssue() {
    detail.value = null;
    loadList();
}

// 评论
const commentText = ref('');
const commentBusy = ref(false);
async function sendComment() {
    if (!detail.value) return;
    const text = commentText.value.trim();
    if (!text) return;
    commentBusy.value = true;
    try {
        await fbIssueComment({ num: detail.value.num, text });
        commentText.value = '';
        detail.value = await fbIssueGet(detail.value.num);
    } catch (e) {
        toast(`评论失败：${e}`, 'error');
    } finally {
        commentBusy.value = false;
    }
}
async function retractComment(c: FBComment) {
    if (!detail.value) return;
    pendingConfirm.value = {
        title: '撤回评论',
        message: '撤回后这条评论不再展示，记录仍保留。',
        run: async () => {
            if (!detail.value) return;
            await fbIssueRetract(detail.value.num, c.id);
            detail.value = await fbIssueGet(detail.value.num);
        }
    };
}
// ---------------- 二次确认（复用 ConfirmModal，不再用原生 confirm）----------------
/** 危险操作的统一确认：title/message + 实际执行体。失败原因就地显示，不吞掉。 */
const pendingConfirm = ref<{ title: string; message: string; run: () => Promise<void> } | null>(null);
const confirmBusy = ref(false);
const confirmError = ref('');
async function runPendingConfirm() {
    const p = pendingConfirm.value;
    if (!p) return;
    confirmBusy.value = true;
    confirmError.value = '';
    try {
        await p.run();
        pendingConfirm.value = null;
    } catch (e) {
        // 失败留在弹层里给原因和重试机会，而不是关掉后只闪一句 toast
        confirmError.value = String(e);
    } finally {
        confirmBusy.value = false;
    }
}

async function deleteIssue() {
    if (!detail.value) return;
    const num = detail.value.num;
    pendingConfirm.value = {
        title: '撤回这条反馈',
        message: '撤回后不再展示在列表里，但记录会保留，管理员仍可查阅。',
        run: async () => {
            await fbIssueDelete(num);
            toast('已撤回（记录保留，管理员仍可查阅）', 'success');
            closeIssue();
        }
    };
}

// 附件预览
const attView = ref<{ url: string; name: string; text?: string } | null>(null);
const attBusy = ref('');
async function openAttachment(kind: 'issue' | 'ticket', owner: string | number, fid: string, name: string) {
    attBusy.value = fid;
    try {
        const r = await fbAttachment(kind, String(owner), fid);
        const url = managedImageUrl(r.path);
        const ext = name.toLowerCase().split('.').pop() || '';
        if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'].includes(ext)) {
            attView.value = { url, name };
        } else {
            // 文本类（日志/崩溃报告）：经 asset 协议读回内容，弹层内滚动展示
            let text = '';
            try {
                const resp = await fetch(url);
                const raw = await resp.text();
                text = raw.length > 200000 ? raw.slice(0, 200000) + '\n…（截断，完整内容见缓存文件）' : raw;
            } catch {
                text = '（无法读取文本内容）';
            }
            attView.value = { url, name, text };
        }
    } catch (e) {
        toast(`附件下载失败：${e}`, 'error');
    } finally {
        attBusy.value = '';
    }
}

// ---------------- 提交 ----------------
const showSubmit = ref(false);
const subKind = ref('');
const subTitle = ref('');
const subBody = ref('');
const subWithLogs = ref(true);
const subFiles = ref<string[]>([]);
const logsPreview = ref<{ name: string; size: number }[]>([]);
const submitBusy = ref(false);
const submitErr = ref('');

function openSubmit() {
    if (!loggedIn.value) {
        showLoginHint.value = true;
        return;
    }
    showSubmit.value = true;
    subKind.value = '';
    subTitle.value = '';
    subBody.value = '';
    subWithLogs.value = true;
    subFiles.value = [];
    submitErr.value = '';
    fbLogsPreview()
        .then((l) => (logsPreview.value = l))
        .catch(() => (logsPreview.value = []));
}
const currentKind = computed(() => meta.value?.kinds.find((k) => k.key === subKind.value) || null);
function pickKind(k: string) {
    subKind.value = k;
    if (k === 'suggestion') subWithLogs.value = false;
}
async function addFile() {
    const paths = await fbPickFiles();
    for (const p of paths) {
        if (!subFiles.value.includes(p)) subFiles.value.push(p);
    }
}
/**
 * 拖入附件（UX 准则：拖拽 > 选择，同一入口两种来源）。
 * 拖放区自己吃掉 drop 事件，避免冒泡到 App 的全局导入流程把日志当整合包处理。
 */
const dropHover = ref(false);
function onAttDrop(e: DragEvent) {
    dropHover.value = false;
    e.preventDefault();
    e.stopPropagation();
    const files = Array.from(e.dataTransfer?.files ?? []);
    for (const f of files) {
        const p = window.kamucl.getFilePath(f);
        if (p && !subFiles.value.includes(p)) subFiles.value.push(p);
    }
    if (!files.length) toast('没有识别到文件，可以改用「选择文件」', 'info');
}
function onAttDragOver(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    dropHover.value = true;
}
function onAttDragLeave(e: DragEvent) {
    e.stopPropagation();
    // 只在自己身上离开才复位，进子元素不算
    if (e.target === e.currentTarget) dropHover.value = false;
}
async function doSubmit() {
    submitErr.value = '';
    if (!subKind.value) return (submitErr.value = '请先选择反馈类型');
    if (!subTitle.value.trim()) return (submitErr.value = '请填写标题');
    if (!subBody.value.trim()) return (submitErr.value = '请填写详细描述');
    const kd = currentKind.value;
    if (kd?.logMode === 'required' && !subWithLogs.value && subFiles.value.length === 0)
        return (submitErr.value = `「${kd.name}」类型必须附带游戏日志`);
    if (kd?.logMode === 'none' && (subFiles.value.length > 0 || subWithLogs.value))
        return (submitErr.value = `「${kd.name}」类型不需要附件`);
    submitBusy.value = true;
    try {
        const r = await fbIssueSubmit({
            title: subTitle.value.trim(),
            body: subBody.value.trim(),
            kind: subKind.value,
            withLogs: subWithLogs.value,
            files: subFiles.value
        });
        showSubmit.value = false;
        toast(`已发布 #${r.num}`, 'success');
        page.value = 1;
        await loadList();
        openIssue(r.num);
    } catch (e) {
        submitErr.value = String(e);
    } finally {
        submitBusy.value = false;
    }
}

// ---------------- 匿名私信 ----------------
const tickets = ref<FBTicketLocal[]>([]);
const ticketDetail = ref<FBTicketDetail | null>(null);
const ticketReplyText = ref('');
const ticketReplyLogs = ref(false);
const ticketBusy = ref(false);
const showTicketNew = ref(false);
const tkTitle = ref('');
const tkDesc = ref('');
const tkKind = ref('other');
const tkLogs = ref(false);
const tkReceipt = ref<{ id: string; secret: string } | null>(null);

const unreadTickets = computed(() => tickets.value.filter((t) => t.hasUnread && !t.deleted).length);

async function loadTickets() {
    try {
        tickets.value = await fbTicketsList();
        const r = await fbTicketsPoll();
        tickets.value = await fbTicketsList();
        for (const row of r.tickets || []) {
            const t = tickets.value.find((x) => x.id === row.id);
            if (t) {
                t.hasUnread = row.hasUnread;
                t.replyCount = row.replyCount;
                t.lastTimeMs = (row.lastTime || 0) * 1000;
            }
        }
    } catch (e) {
        /* 离线不打扰 */
    }
}
async function openTicket(id: string) {
    ticketBusy.value = true;
    ticketDetail.value = null;
    try {
        ticketDetail.value = await fbTicketDetail(id);
        if (ticketDetail.value) {
            await fbTicketViewed(id);
            const t = tickets.value.find((x) => x.id === id);
            if (t) t.hasUnread = false;
        }
    } catch (e) {
        toast(`读取失败：${e}`, 'error');
    } finally {
        ticketBusy.value = false;
    }
}
async function sendTicketReply() {
    const d = ticketDetail.value;
    if (!d) return;
    const text = ticketReplyText.value.trim();
    if (!text && !ticketReplyLogs.value) return;
    ticketBusy.value = true;
    try {
        await fbTicketReply({ id: d.id, text, withLogs: ticketReplyLogs.value });
        ticketReplyText.value = '';
        ticketReplyLogs.value = false;
        ticketDetail.value = await fbTicketDetail(d.id);
        toast('已发送', 'success');
    } catch (e) {
        toast(`发送失败：${e}`, 'error');
    } finally {
        ticketBusy.value = false;
    }
}
async function retractTicketMsg(id: string, msg: string) {
    pendingConfirm.value = {
        title: '撤回追问',
        message: '撤回后这条追问不再展示给管理员，记录仍保留。',
        run: async () => {
            await fbTicketRetract(id, msg);
            ticketDetail.value = await fbTicketDetail(id);
        }
    };
}
async function deleteTicket(id: string) {
    pendingConfirm.value = {
        title: '删除本机记录',
        message: '只影响这台设备：服务端仍保留记录（单向删除），管理员彻底删除后这里也会同步清掉。单号与密钥将从本机移除，之后无法再查询这条私信。',
        run: async () => {
            await fbTicketDelete(id);
            tickets.value = await fbTicketsList();
            ticketDetail.value = null;
            toast('已删除本机记录', 'success');
        }
    };
}
/**
 * 公开板表单 → 匿名私信：把已填的内容带过去。
 * UX 准则里"降低试错成本"要求错了能改而不重填；原来切换会清空标题与正文。
 */
function toPrivate() {
    if (!tkTitle.value) tkTitle.value = subTitle.value;
    if (!tkDesc.value) tkDesc.value = subBody.value;
    if (subKind.value) tkKind.value = subKind.value;
    showSubmit.value = false;
    showLoginHint.value = false;
    tab.value = 'tickets';
    showTicketNew.value = true;
    submitErr.value = '';
}

async function submitTicket() {
    const desc = tkDesc.value.trim();
    if (!desc) return (submitErr.value = '请写点内容');
    ticketBusy.value = true;
    try {
        const r = await fbTicketSubmit({ title: tkTitle.value.trim(), description: desc, kind: tkKind.value, withLogs: tkLogs.value });
        tkReceipt.value = r;
        tkTitle.value = '';
        tkDesc.value = '';
        tkLogs.value = false;
        showTicketNew.value = false;
        tickets.value = await fbTicketsList();
    } catch (e) {
        toast(`发送失败：${e}`, 'error');
    } finally {
        ticketBusy.value = false;
    }
}

// ---------------- 登录引导弹层 ----------------
const showLoginHint = ref(false);

/**
 * 切标签时刷新对应列表。只在 mounted 拉一次的话，去私信页转一圈回来
 * 仍是旧数据，新提交的反馈看不见（"不确定感"的典型来源）。
 */
function switchTab(next: Tab) {
    if (tab.value === next) return;
    tab.value = next;
    if (next === 'board') {
        detail.value = null;
        void loadList();
    } else {
        ticketDetail.value = null;
        showTicketNew.value = false;
        void loadTickets();
    }
}

// ---------------- 工具 ----------------
function fmtTime(sec: number): string {
    if (!sec) return '';
    const d = new Date(sec * 1000);
    const p = (n: number) => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}
function fmtSize(b: number): string {
    if (b >= 1048576) return (b / 1048576).toFixed(1) + ' MB';
    if (b >= 1024) return (b / 1024).toFixed(1) + ' KB';
    return b + ' B';
}
function statusColor(key: string): string {
    return meta.value?.statuses.find((s) => s.key === key)?.color || 'var(--text-dim)';
}
function statusName(key: string): string {
    return meta.value?.statuses.find((s) => s.key === key)?.name || key;
}

onMounted(async () => {
    // 后台配对轮询的结果统一从这里回来（成功/失败/取消三态）。
    offLoginDone = window.kamucl.on('event:feedbackLoginDone', (raw) => {
        const p = (raw ?? {}) as { ok?: boolean; cancelled?: boolean; identity?: { name?: string }; error?: string };
        loginBusy.value = false;
        if (p.ok) {
            pairCode.value = '';
            void refreshMeta().then(() => {
                toast(`已登录：${p.identity?.name ?? ''}`, 'success');
                loadList();
            });
            return;
        }
        if (p.cancelled) return; // 用户主动取消，不额外打扰
        pairCode.value = '';
        toast(`登录失败：${p.error ?? '未知原因'}`, 'error');
    });
    await refreshMeta();
    await loadList();
    await loadTickets();
});
let offLoginDone: (() => void) | null = null;
onUnmounted(() => {
    offLoginDone?.();
    offLoginDone = null;
});
</script>

<template>
    <section class="fb-page" data-ui="feedback:page">
        <header class="page-head fb-heading" data-ui="feedback:heading">
            <div>
                <h1>
                    反馈与建议
                    <small v-if="tab === 'board'">公开板 {{ total }} 条</small>
                    <small v-else>匿名私信 {{ tickets.filter((t) => !t.deleted).length }} 条</small>
                </h1>
                <p class="muted">Bug、崩溃、功能建议直达开发者；不方便实名的可以走匿名私信。</p>
            </div>
            <div class="actions">
                <div class="fb-tabs">
                    <button class="fb-tab" :class="{ active: tab === 'board' }" @click="switchTab('board')">公开板</button>
                    <button class="fb-tab" :class="{ active: tab === 'tickets' }" @click="switchTab('tickets')">
                        匿名私信<span v-if="unreadTickets" class="fb-dot"></span>
                    </button>
                </div>
                <template v-if="loggedIn">
                    <span class="muted">{{ meta?.identity?.name }}</span>
                    <button class="btn btn-ghost btn-sm" @click="doLogout">退出</button>
                </template>
                <!-- 等待授权期间：把配对码摆出来供核对，并给取消出口 -->
                <template v-else-if="loginBusy">
                    <span class="fb-paircode">
                        配对码
                        <code v-if="pairCode">{{ pairCode }}</code>
                        <span v-else class="muted">获取中…</span>
                    </span>
                    <button class="btn btn-ghost btn-sm" @click="cancelLogin">取消登录</button>
                </template>
                <button v-else class="btn btn-gold btn-sm" :disabled="loginBusy" @click="doLogin">
                    <svg class="ms-svg" viewBox="0 0 23 23" aria-hidden="true"><rect x="1" y="1" width="10" height="10" fill="#f25022"/><rect x="12" y="1" width="10" height="10" fill="#7fba00"/><rect x="1" y="12" width="10" height="10" fill="#00a4ef"/><rect x="12" y="12" width="10" height="10" fill="#ffb900"/></svg>
                    微软登录
                </button>
            </div>
        </header>

        <!-- ============ 公开板 ============ -->
        <template v-if="tab === 'board'">
            <!-- 详情 -->
            <template v-if="detail">
                <div class="fb-detailbar">
                    <button class="btn btn-ghost btn-sm" @click="closeIssue">← 返回列表</button>
                    <button v-if="detail.canDelete" class="btn btn-ghost btn-sm" @click="deleteIssue">
                        撤回这条反馈
                    </button>
                </div>
                <div class="card">
                    <h2 class="fb-detail-title"><span class="fb-num">#{{ detail.num }}</span>{{ detail.title }}</h2>
                    <div class="fb-metline">
                        <span class="tag" :style="{ background: detail.kindColor, color: '#fff' }">{{ detail.kindName }}</span>
                        <span class="tag" :style="{ color: statusColor(detail.status), borderColor: statusColor(detail.status) }">{{ statusName(detail.status) }}</span>
                        <span class="muted">{{ detail.author }} · {{ fmtTime(detail.time) }}</span>
                    </div>
                    <p class="fb-body">{{ detail.body }}</p>
                    <details v-if="detail.context" class="collapse">
                        <summary class="collapse-head"><span class="group-title">客户端环境信息</span></summary>
                        <div class="fb-ctx-body"><pre>{{ JSON.stringify(detail.context, null, 2) }}</pre></div>
                    </details>
                    <div v-if="detail.attachments?.length" class="fb-atts">
                        <button v-for="a in detail.attachments" :key="a.fid" class="tag" :disabled="attBusy === a.fid" @click="openAttachment('issue', detail.num, a.fid, a.name)">
                            <UiGlyph name="clip" :size="12" /> {{ a.name }} <small>{{ fmtSize(a.size) }}</small>
                        </button>
                    </div>
                </div>

                <div class="card">
                    <h3 class="fb-sec">评论（{{ detail.comments.length }}）</h3>
                    <div v-for="c in detail.comments" :key="c.id" class="fb-comment" :class="{ admin: c.from === 'admin' }">
                        <div class="fb-avatar">{{ (c.author || '?').slice(0, 1).toUpperCase() }}</div>
                        <div class="fb-comment-main">
                            <div class="fb-comment-head">
                                <strong>{{ c.author }}</strong>
                                <span v-if="c.from === 'admin'" class="tag tag-gold">官方</span>
                                <span class="muted">{{ fmtTime(c.time) }}</span>
                                <button v-if="c.mine" class="fb-linkbtn" @click="retractComment(c)">撤回</button>
                            </div>
                            <p>{{ c.text }}</p>
                            <div v-if="c.attachments?.length" class="fb-atts">
                                <button v-for="a in c.attachments" :key="a.fid" class="tag" @click="openAttachment('issue', detail.num, a.fid, a.name)"><UiGlyph name="clip" :size="12" /> {{ a.name }}</button>
                            </div>
                        </div>
                    </div>
                    <p v-if="!detail.comments.length" class="muted" style="padding: 8px 0">还没有评论。</p>
                    <div v-if="detail.canComment" class="fb-comment-form">
                        <textarea v-model="commentText" class="input" rows="2" maxlength="2000" placeholder="说点什么…支持补充复现步骤"></textarea>
                        <button class="btn btn-gold btn-sm" :disabled="commentBusy || !commentText.trim()" @click="sendComment">
                            {{ commentBusy ? '发送中…' : '发表评论' }}
                        </button>
                    </div>
                    <button v-else class="btn btn-gold btn-sm" style="margin-top: 10px" @click="showLoginHint = true">
                        <svg class="ms-svg" viewBox="0 0 23 23"><rect x="1" y="1" width="10" height="10" fill="#f25022"/><rect x="12" y="1" width="10" height="10" fill="#7fba00"/><rect x="1" y="12" width="10" height="10" fill="#00a4ef"/><rect x="12" y="12" width="10" height="10" fill="#ffb900"/></svg>
                        登录后评论
                    </button>
                </div>
            </template>

            <!-- 列表 -->
            <template v-else>
                <div class="card fb-filtercard">
                    <div class="fb-filters">
                        <SelectMenu v-model="filterKind" :options="kindOptions" @update:model-value="refilter" />
                        <SelectMenu v-model="filterStatus" :options="statusOptions" @update:model-value="refilter" />
                        <input v-model="keyword" class="input" placeholder="搜索标题与内容…" @keyup.enter="refilter" />
                        <button class="btn btn-ghost btn-sm" @click="refilter">搜索</button>
                        <button class="btn btn-sm" :class="mineOnly ? 'btn-gold' : 'btn-ghost'" @click="loggedIn ? ((mineOnly = !mineOnly), refilter()) : (showLoginHint = true)">只看我的</button>
                        <button class="btn btn-gold btn-sm" @click="openSubmit">＋ 提交反馈</button>
                    </div>
                </div>

                <div v-if="listLoading" class="muted" style="padding: 24px; text-align: center">加载中…</div>
                <template v-else>
                    <div v-if="!issues.length" class="card empty">
                        <p>{{ mineOnly ? '你还没有发过反馈。第一条会出现在这里，处理进度一目了然。' : filterKind || filterStatus || keyword ? '没有符合条件的反馈，换个筛选试试。' : '这里会展示玩家提交的反馈与建议，现在就可以发出第一条。' }}</p>
                        <button v-if="mineOnly || filterKind || filterStatus || keyword" class="btn btn-ghost btn-sm" @click="mineOnly = false; filterKind = ''; filterStatus = ''; keyword = ''; refilter()">清空筛选</button>
                        <button v-else class="btn btn-gold btn-sm" @click="openSubmit">＋ 提交反馈</button>
                    </div>
                    <button v-for="it in issues" :key="it.num" class="card fb-row" @click="openIssue(it.num)">
                        <span class="fb-num">#{{ it.num }}</span>
                        <span class="fb-row-main">
                            <span class="fb-row-title">{{ it.title }}</span>
                            <span class="fb-row-meta">
                                <span class="tag" :style="{ background: it.kindColor, color: '#fff' }">{{ it.kindName }}</span>
                                <span class="tag" :style="{ color: it.statusColor, borderColor: it.statusColor }">{{ it.statusName }}</span>
                                <span class="muted">{{ it.author }} · {{ fmtTime(it.time) }}</span>
                                <span v-if="it.comments" class="muted"><UiGlyph name="comment" :size="12" /> {{ it.comments }}</span>
                                <span v-if="it.attachments" class="muted"><UiGlyph name="clip" :size="12" /> {{ it.attachments }}</span>
                            </span>
                        </span>
                    </button>
                    <div v-if="total > pageSize" class="fb-pager">
                        <button class="btn btn-ghost btn-sm" :disabled="page <= 1" @click="turnPage(-1)">上一页</button>
                        <span class="muted">第 {{ page }} / {{ totalPages }} 页 · 共 {{ total }} 条</span>
                        <button class="btn btn-ghost btn-sm" :disabled="page >= totalPages" @click="turnPage(1)">下一页</button>
                    </div>
                </template>
            </template>
        </template>

        <!-- ============ 匿名私信 ============ -->
        <template v-if="tab === 'tickets'">
            <div v-if="tkReceipt" class="card fb-receipt">
                <h3>私信已送达管理员</h3>
                <p class="muted">请务必保存单号和密钥——查回复、追问都只认它们；换设备后只有凭这一对才能找回会话。</p>
                <div class="fb-receipt-row"><label>单号</label><code>{{ tkReceipt.id }}</code></div>
                <div class="fb-receipt-row"><label>密钥（只显示这一次）</label><code class="fb-secret">{{ tkReceipt.secret }}</code></div>
                <button class="btn btn-gold btn-sm" @click="tkReceipt = null">好的，已保存</button>
            </div>

            <template v-else-if="ticketDetail">
                <div class="fb-detailbar">
                    <button class="btn btn-ghost btn-sm" @click="ticketDetail = null">← 返回私信列表</button>
                    <button class="btn btn-ghost btn-sm" @click="deleteTicket(ticketDetail.id)">
                        删除本机记录
                    </button>
                </div>
                <div class="card">
                    <h2 class="fb-detail-title"><span class="fb-num mono">{{ ticketDetail.id }}</span>{{ ticketDetail.title || '私信' }}</h2>
                    <div class="fb-metline">
                        <span class="muted">{{ fmtTime(ticketDetail.time) }}</span>
                        <span v-if="ticketDetail.publishedTo" class="tag tag-cyan">已转公开 #{{ ticketDetail.publishedTo }}</span>
                    </div>
                    <div class="fb-thread">
                        <div class="fb-msg player">
                            <div class="fb-comment-head"><strong>我</strong><span class="muted">{{ fmtTime(ticketDetail.time) }}</span></div>
                            <p class="fb-body" style="margin: 4px 0 0">{{ ticketDetail.description }}</p>
                            <div v-if="ticketDetail.attachments?.length" class="fb-atts">
                                <button v-for="a in ticketDetail.attachments" :key="a.fid" class="tag" @click="openAttachment('ticket', ticketDetail.id, a.fid, a.name)"><UiGlyph name="clip" :size="12" /> {{ a.name }} <small>{{ fmtSize(a.size) }}</small></button>
                            </div>
                        </div>
                        <div v-for="m in ticketDetail.messages" :key="m.id" class="fb-msg" :class="m.from">
                            <div class="fb-comment-head">
                                <strong>{{ m.from === 'admin' ? '管理员' : '我' }}</strong>
                                <span v-if="m.from === 'admin'" class="tag tag-gold">官方</span>
                                <span class="muted">{{ fmtTime(m.time) }}</span>
                                <button v-if="m.from === 'player'" class="fb-linkbtn" @click="retractTicketMsg(ticketDetail.id, m.id)">撤回</button>
                            </div>
                            <p>{{ m.text }}</p>
                            <div v-if="m.attachments?.length" class="fb-atts">
                                <button v-for="a in m.attachments" :key="a.fid" class="tag" @click="openAttachment('ticket', ticketDetail.id, a.fid, a.name)"><UiGlyph name="clip" :size="12" /> {{ a.name }}</button>
                            </div>
                        </div>
                    </div>
                    <div class="fb-comment-form">
                        <textarea v-model="ticketReplyText" class="input" rows="2" maxlength="2000" placeholder="追加内容：补充说明、新版本是否复现…"></textarea>
                        <label class="fb-check"><input v-model="ticketReplyLogs" type="checkbox" />附带当前实例日志</label>
                        <button class="btn btn-gold btn-sm" :disabled="ticketBusy" @click="sendTicketReply">发送</button>
                    </div>
                </div>
            </template>

            <template v-else>
                <div class="fb-detailbar">
                    <p class="muted" style="margin: 0; flex: 1">匿名私信不登录、不上公开板，直达管理员。适合敏感反馈（举报外挂/安全问题）。</p>
                    <button class="btn btn-gold btn-sm" @click="showTicketNew = true; submitErr = ''">＋ 新建私信</button>
                </div>
                <div v-if="showTicketNew" class="card">
                    <h3>新建匿名私信</h3>
                    <input v-model="tkTitle" class="input" maxlength="120" placeholder="标题（可选）" style="margin-top: 10px" />
                    <textarea v-model="tkDesc" class="input" rows="5" maxlength="10000" placeholder="尽量写清楚：版本、复现步骤、期望结果。" style="margin-top: 8px"></textarea>
                    <div class="fb-filters" style="margin-top: 8px">
                        <SelectMenu v-model="tkKind" :options="(meta?.kinds || []).map((k) => ({ value: k.key, label: k.name }))" />
                    </div>
                    <label class="fb-check"><input v-model="tkLogs" type="checkbox" />附带当前实例日志</label>
                    <p v-if="submitErr" class="fb-err">{{ submitErr }}</p>
                    <div class="modal-actions" style="margin-top: 12px">
                        <button class="btn btn-ghost btn-sm" @click="showTicketNew = false">取消</button>
                        <button class="btn btn-gold btn-sm" :disabled="ticketBusy" @click="submitTicket">发送</button>
                    </div>
                </div>
                <div v-if="!tickets.filter((t) => !t.deleted).length && !showTicketNew" class="card empty">
                    <p>这台设备还没有私信记录。不方便实名、或内容敏感时走这里。</p>
                </div>
                <!-- v-for 与 v-else-if 混写会让列表挂在空状态判断上，这里拆成独立分支 -->
                <template v-else-if="!showTicketNew">
                    <button v-for="t in tickets.filter((x) => !x.deleted)" :key="t.id" class="card fb-row" @click="openTicket(t.id)">
                        <span class="fb-num mono">{{ t.id }}</span>
                        <span class="fb-row-main">
                            <span class="fb-row-title">{{ t.title || '私信 ' + t.id }}</span>
                            <span class="fb-row-meta"><span class="muted">{{ fmtTime(Math.round(t.lastTimeMs / 1000)) }}</span></span>
                        </span>
                        <span class="fb-row-right">
                            <span v-if="t.hasUnread" class="tag tag-danger">有新回复</span>
                            <span v-if="t.replyCount" class="muted"><UiGlyph name="comment" :size="12" /> {{ t.replyCount }}</span>
                        </span>
                    </button>
                </template>
            </template>
        </template>

        <!-- 提交反馈弹层 -->
        <div v-if="showSubmit" class="modal-mask" @click.self="showSubmit = false">
            <div class="modal fb-modal">
                <h3>提交反馈</h3>
                <p class="muted" style="margin: 4px 0 10px">选一个类型。崩溃 / Bug 需要附上游戏日志，帮开发者快速定位。</p>
                <div class="fb-kind-grid">
                    <button v-for="k in meta?.kinds || []" :key="k.key" class="fb-kind" :class="{ sel: subKind === k.key }" @click="pickKind(k.key)">
                        <span class="tag" :style="{ background: k.color, color: '#fff' }">{{ k.name }}</span>
                        <small>{{ k.logMode === 'required' ? '需附日志' : k.logMode === 'none' ? '无需日志' : '日志可选' }}</small>
                    </button>
                </div>
                <p v-if="currentKind?.logMode === 'required'" class="fb-hint">该类型必须附带日志：当前实例的崩溃报告与 latest.log 会自动带上（见下方预览）。</p>
                <p v-else-if="currentKind?.logMode === 'none'" class="fb-hint">建议类不需要日志，描述清楚想要什么就好。</p>
                <input v-model="subTitle" class="input" maxlength="120" placeholder="标题：一句话说清问题" style="margin-top: 8px" />
                <textarea v-model="subBody" class="input" rows="5" maxlength="10000" placeholder="发生了什么？期望是什么？怎么复现？越具体越好修。" style="margin-top: 8px"></textarea>
                <template v-if="currentKind && currentKind.logMode !== 'none'">
                    <label class="fb-check"><input v-model="subWithLogs" type="checkbox" />附带当前实例日志</label>
                    <div v-if="subWithLogs" class="fb-logpreview">
                        <span v-for="l in logsPreview" :key="l.name" class="muted"><UiGlyph name="file" :size="12" /> {{ l.name }}（{{ fmtSize(l.size) }}）</span>
                        <span v-if="!logsPreview.length" class="muted">未找到日志文件（先打开一个实例再试）</span>
                    </div>
                    <div class="fb-files">
                        <span v-for="(f, i) in subFiles" :key="f" class="tag">{{ String(f).split(/[\\/]/).pop() }}<button class="fb-linkbtn" aria-label="移除该附件" @click="subFiles.splice(i, 1)"><UiGlyph name="close" :size="11" /></button></span>
                        <button
                            class="fb-drop"
                            :class="{ sel: dropHover }"
                            @click="addFile"
                            @dragover="onAttDragOver"
                            @dragleave="onAttDragLeave"
                            @drop="onAttDrop"
                        >
                            ＋ 添加附件（可点选，也可把文件拖到这里）
                        </button>
                    </div>
                </template>
                <p v-if="submitErr" class="fb-err">{{ submitErr }}</p>
                <div class="modal-actions">
                    <button class="btn btn-ghost btn-sm" @click="toPrivate">改发匿名私信</button>
                    <button class="btn btn-gold btn-sm" :disabled="submitBusy" @click="doSubmit">{{ submitBusy ? '提交中…' : '发布到公开板' }}</button>
                </div>
            </div>
        </div>

        <!-- 登录引导弹层 -->
        <div v-if="showLoginHint" class="modal-mask" @click.self="showLoginHint = false">
            <div class="modal" style="max-width: 400px; text-align: center">
                <h3>登录后参与社区</h3>
                <p class="muted">公开板的提交与评论需要微软账号（有邮箱即可，不需要正版 Minecraft 账号）。也可以改为<strong>匿名私信</strong>，只发给管理员。</p>
                <div style="display: flex; flex-direction: column; gap: 8px; margin-top: 14px">
                    <button
                        v-if="!loginBusy"
                        class="btn btn-gold"
                        @click="showLoginHint = false; doLogin()"
                    >
                        <svg class="ms-svg" viewBox="0 0 23 23"><rect x="1" y="1" width="10" height="10" fill="#f25022"/><rect x="12" y="1" width="10" height="10" fill="#7fba00"/><rect x="1" y="12" width="10" height="10" fill="#00a4ef"/><rect x="12" y="12" width="10" height="10" fill="#ffb900"/></svg>
                        微软账号登录
                    </button>
                    <!-- 已发起：显示配对码，让用户在网页上逐位核对后再批准 -->
                    <div v-else class="fb-paircode" style="justify-content: center">
                        配对码
                        <code v-if="pairCode">{{ pairCode }}</code>
                        <span v-else class="muted">获取中…</span>
                    </div>
                    <button v-if="loginBusy" class="btn btn-ghost" @click="showLoginHint = false; cancelLogin()">取消登录</button>
                    <button v-else class="btn btn-ghost" @click="toPrivate">改为匿名私信</button>
                </div>
            </div>
        </div>

        <!-- 危险操作二次确认：统一走项目既有 ConfirmModal（不再用原生 confirm） -->
        <ConfirmModal
            :open="!!pendingConfirm"
            :title="pendingConfirm?.title ?? ''"
            :message="pendingConfirm?.message ?? ''"
            :busy="confirmBusy"
            :error="confirmError"
            confirm-text="确认"
            @confirm="runPendingConfirm"
            @cancel="pendingConfirm = null; confirmError = ''"
        />

        <!-- 附件预览弹层 -->
        <div v-if="attView" class="modal-mask" @click.self="attView = null">
            <div class="modal" style="max-width: 720px">
                <h3>{{ attView.name }}</h3>
                <img v-if="attView.text === undefined" :src="attView.url" style="max-width: 100%; border-radius: var(--radius-md)" />
                <pre v-else class="fb-textview">{{ attView.text }}</pre>
                <div class="modal-actions"><button class="btn btn-ghost btn-sm" @click="attView = null">关闭</button></div>
            </div>
        </div>
    </section>
</template>

<style scoped>
/* 布局用页面前缀类；颜色/字号/圆角/间距一律消费 styles.css 令牌。
   微软四格图标是品牌标识，色值例外。 */
.fb-page {
    max-width: 1120px;
    margin: 0 auto;
    display: grid;
    gap: 12px;
    min-width: 0;
}
.fb-heading {
    display: flex;
    flex-direction: row;
    text-align: left;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
}
.fb-heading h1 {
    margin: 0 0 8px;
    font-size: 24px;
}
.fb-heading h1 small {
    font-size: 13px;
    font-weight: 400;
    color: var(--text-dim);
}
.fb-heading p {
    margin: 4px 0;
}
.actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
}
.fb-tabs {
    display: inline-flex;
    gap: 4px;
    padding: 4px;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 999px;
}
.fb-tab {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 14px;
    border-radius: 999px;
    font-size: var(--text-sm);
    color: var(--text-dim);
    background: transparent;
    border: none;
    cursor: pointer;
    transition: background var(--motion-fast), color var(--motion-fast);
}
.fb-tab:hover {
    color: var(--text);
}
.fb-tab.active {
    background: var(--accent-soft);
    color: var(--accent-2);
    font-weight: 600;
}
.fb-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--danger);
}
/* 配对码需要一眼看清、方便与网页上显示的码逐位核对 */
.fb-paircode {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-xs);
    color: var(--text-dim);
}
.fb-paircode code {
    font-family: var(--mono);
    font-size: var(--text-md);
    letter-spacing: 2px;
    color: var(--text);
    background: var(--card-2);
    padding: 2px 8px;
    border-radius: var(--radius-sm);
}
.ms-svg {
    width: 14px;
    height: 14px;
    vertical-align: -2px;
}
.fb-detailbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
}
.fb-detail-title {
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--text);
    margin: 0 0 8px;
    display: flex;
    gap: 10px;
    align-items: baseline;
}
.fb-num {
    font-family: var(--mono);
    font-size: var(--text-sm);
    color: var(--text-dim);
}
.fb-num.mono {
    font-size: var(--text-md);
}
.fb-metline {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
    margin-bottom: 12px;
}
.fb-body {
    white-space: pre-wrap;
    word-break: break-word;
    font-size: var(--text-md);
    color: var(--text);
    margin: 0 0 10px;
}
.fb-ctx-body pre {
    font-size: var(--text-xs);
    background: var(--card-2);
    padding: var(--space-2);
    border-radius: var(--radius-sm);
    overflow: auto;
    margin: 0;
}
.fb-sec {
    font-size: var(--text-md);
    color: var(--text);
    margin: 0 0 8px;
}
.fb-atts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 8px 0;
}
.fb-atts .tag {
    cursor: pointer;
}
.fb-atts .tag:hover {
    border-color: var(--accent);
}
.fb-comment {
    display: flex;
    gap: var(--space-3);
    padding: var(--space-3) 0;
    border-top: 1px solid var(--border);
}
.fb-comment:first-of-type {
    border-top: none;
}
.fb-avatar {
    width: 34px;
    height: 34px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--accent-soft);
    color: var(--accent-2);
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 700;
    font-size: var(--text-sm);
}
.fb-comment.admin .fb-avatar {
    background: var(--ok-soft);
    color: var(--ok);
}
.fb-comment-main {
    flex: 1;
    min-width: 0;
}
.fb-comment-main p {
    white-space: pre-wrap;
    word-break: break-word;
    font-size: var(--text-sm);
    color: var(--text);
    margin: 4px 0 0;
}
.fb-comment-head {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: var(--text-xs);
    color: var(--text-dim);
}
.fb-comment-head strong {
    color: var(--text);
}
.fb-linkbtn {
    background: none;
    border: none;
    color: var(--accent-2);
    font-size: var(--text-xs);
    cursor: pointer;
    padding: 0;
}
.fb-linkbtn:hover {
    text-decoration: underline;
}
.fb-comment-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
    margin-top: var(--space-2);
}
.fb-thread {
    display: flex;
    flex-direction: column;
}
.fb-msg {
    padding: var(--space-3) 0;
    border-top: 1px solid var(--border);
}
.fb-msg:first-child {
    border-top: none;
}
.fb-msg p {
    white-space: pre-wrap;
    word-break: break-word;
    margin: 4px 0 0;
    font-size: var(--text-sm);
    color: var(--text);
}
.fb-msg.admin {
    background: var(--accent-soft);
    border-radius: var(--radius-md);
    padding: var(--space-3);
    margin: 4px 0;
    border-top: none;
}
.fb-filtercard {
    padding: var(--space-3) var(--card-pad);
}
.fb-filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
}
.fb-filters .input {
    flex: 1;
    min-width: 180px;
}
.fb-row {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    text-align: left;
    cursor: pointer;
    width: 100%;
    padding: var(--space-3) var(--card-pad);
    transition: border-color var(--motion-fast), transform var(--motion-fast);
}
.fb-row:hover {
    border-color: var(--accent);
    transform: translateY(-1px);
}
.fb-row-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
}
.fb-row-title {
    font-size: var(--text-md);
    font-weight: 600;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.fb-row-meta {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
    font-size: var(--text-xs);
}
.fb-row-right {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
}
.fb-pager {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
}
.fb-kind-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
}
.fb-kind {
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: center;
    padding: 10px 6px;
    border-radius: var(--radius-md);
    background: var(--card-2);
    border: 2px solid transparent;
    cursor: pointer;
    transition: border-color var(--motion-fast);
    color: var(--text-dim);
}
.fb-kind.sel {
    border-color: var(--accent);
}
.fb-hint {
    margin: 8px 0 0;
    padding: 8px 12px;
    font-size: var(--text-xs);
    background: var(--accent-soft);
    border-left: 3px solid var(--accent);
    border-radius: var(--radius-sm);
    color: var(--text);
}
.fb-check {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-xs);
    color: var(--text-dim);
    margin-top: var(--space-2);
    cursor: pointer;
}
.fb-logpreview {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    font-size: var(--text-xs);
    padding: 2px 0 0 22px;
}
.fb-files {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    margin-top: var(--space-2);
}
/* 拖放区：悬停/拖入时明确高亮，符合"悬浮反馈"要求 */
.fb-drop {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    font-size: var(--text-xs);
    color: var(--text-dim);
    background: var(--card-2);
    border: 1px dashed var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: border-color var(--motion-fast), color var(--motion-fast), background var(--motion-fast);
}
.fb-drop:hover {
    color: var(--text);
    border-color: var(--accent);
}
.fb-drop.sel {
    color: var(--accent-2);
    background: var(--accent-soft);
    border-color: var(--accent);
}
.fb-err {
    color: var(--danger);
    font-size: var(--text-xs);
    margin: var(--space-2) 0 0;
}
.fb-modal {
    width: min(640px, calc(100vw - 48px));
}
.fb-receipt {
    text-align: center;
}
.fb-receipt-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    justify-content: center;
    margin: var(--space-2) 0;
}
.fb-receipt-row label {
    font-size: var(--text-xs);
    color: var(--text-dim);
}
.fb-receipt-row code {
    background: var(--card-2);
    padding: 4px 10px;
    border-radius: var(--radius-sm);
    font-family: var(--mono);
    font-size: var(--text-sm);
    color: var(--text);
    word-break: break-all;
}
.fb-secret {
    font-size: var(--text-xs) !important;
}
.fb-textview {
    max-height: 60vh;
    overflow: auto;
    background: var(--card-2);
    border-radius: var(--radius-sm);
    padding: var(--space-3);
    font-family: var(--mono);
    font-size: var(--text-xs);
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-all;
    color: var(--text);
    margin: 0;
}
@media (max-width: 660px) {
    .fb-kind-grid {
        grid-template-columns: repeat(2, 1fr);
    }
}
</style>
