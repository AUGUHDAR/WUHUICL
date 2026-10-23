<script setup lang="ts">
/** 工单：附件走系统选择框或拖进窗口，列表靠 poll 与服务端对齐。 */
import UiGlyph from '../../UiGlyph.vue';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { toast } from '../../../store';
import { call, onDropPaths, vl } from './useVoxlink';
import './voxlink.css';

type Row = Record<string, unknown>;
type View = 'list' | 'create' | 'detail';

const FILES_MAX = 10;
const BYTES_MAX = 500 * 1024 * 1024;
const DESC_MAX = 10000;
const REPLY_MAX = 2000;
const PAGE = 6;

const view = ref<View>('list');
const tickets = ref<Row[]>([]);
const unread = ref(0);
const detail = ref<Row | null>(null);
const text = ref('');
const files = ref<string[]>([]);
const sizes = ref<Record<string, number>>({});
const busy = ref(false);
const error = ref('');
const page = ref(0);
const msgPage = ref(0);
const dropping = ref(false);

const str = (v: unknown) => (v === undefined || v === null ? '' : String(v));
const stamp = (v: unknown, seconds = false) => {
    const n = Number(v);
    if (!n) return '时间未知';
    return new Date(seconds ? n * 1000 : n).toLocaleString();
};
const base = (p: string) => p.replace(/\\/g, '/').split('/').pop() || p;
const isCreate = computed(() => view.value === 'create');
const pageTickets = computed(() => tickets.value.slice(page.value * PAGE, page.value * PAGE + PAGE));
const msgs = computed<Row[]>(() => asRows(detail.value?.messages));
const msgSlice = computed(() => msgs.value.slice(msgPage.value * 5, msgPage.value * 5 + 5));
const attachments = computed<Row[]>(() => asRows(detail.value?.attachments));
const totalFiles = computed(() => files.value.reduce((sum, p) => sum + (sizes.value[p] || 0), 0));

function asRows(data: unknown): Row[] {
    if (Array.isArray(data)) return data as Row[];
    return [];
}

function state(t: Row): { text: string; cls: string } {
    if (t.hasUnread === true) return { text: '有回复', cls: 'ok' };
    if (Number(t.replyCount) > 0) return { text: `已回复 ${t.replyCount}`, cls: 'busy' };
    return { text: '等待回复', cls: '' };
}

/** 先 poll 让服务端把已删的工单差掉，再读本地列表显示。 */
async function load(): Promise<void> {
    busy.value = true;
    error.value = '';
    const p = await call<Row>('vlcli:tickets', { action: 'poll' });
    if (!p.ok) error.value = p.message || '工单同步失败';
    const r = await call<Row>('vlcli:tickets', { action: 'list' });
    busy.value = false;
    if (!r.ok) {
        error.value = r.message || '工单没读回来';
        return;
    }
    const data = r.data || {};
    tickets.value = asRows(data.tickets ?? data.items ?? data.list);
    unread.value = Number(data.unread || 0);
    page.value = 0;
}

async function open(id: string): Promise<void> {
    busy.value = true;
    const r = await call<Row>('vlcli:tickets', { action: 'detail', id });
    busy.value = false;
    if (!r.ok) {
        error.value = r.message || '详情没读回来';
        return;
    }
    detail.value = r.data || {};
    view.value = 'detail';
    msgPage.value = 0;
    text.value = '';
    files.value = [];
    sizes.value = {};
    void call('vlcli:tickets', { action: 'viewed', id });
}

async function pickFiles() {
    const r = await call<{ files?: string[] }>('vlcli:pickFiles');
    if (!r.ok) {
        error.value = r.message || '选择文件没成功';
        return;
    }
    addPaths((r.data?.files || []) as string[]);
}

function addPaths(paths: string[]) {
    const next = [...files.value];
    for (const p of paths) {
        if (!p || next.includes(p)) continue;
        if (next.length >= FILES_MAX) {
            error.value = '每条消息最多选择 10 个附件';
            break;
        }
        next.push(p);
    }
    files.value = next;
}

async function send(): Promise<void> {
    const body = text.value.trim();
    const id = str(detail.value?.id);
    error.value = '';
    if (!body) {
        error.value = isCreate.value ? '先把问题写清楚' : '先写要追问的内容';
        return;
    }
    if (body.length > (isCreate.value ? DESC_MAX : REPLY_MAX)) {
        error.value = isCreate.value ? '问题描述最长 10000 字' : '追问最长 2000 字';
        return;
    }
    if (totalFiles.value > BYTES_MAX) {
        error.value = '附件总量不得超过 500 MB';
        return;
    }
    busy.value = true;
    const r = await call<Row>(
        'vlcli:tickets',
        isCreate.value
            ? { action: 'submit', description: body, files: files.value }
            : { action: 'reply', id, text: body, files: files.value }
    );
    busy.value = false;
    if (!r.ok) {
        error.value = r.message || '没发出去';
        return;
    }
    toast(isCreate.value ? '工单已提交' : '追问已发送', 'success');
    text.value = '';
    files.value = [];
    if (isCreate.value) {
        view.value = 'list';
        await load();
        return;
    }
    await open(id);
    void load();
}

async function remove(): Promise<void> {
    const id = str(detail.value?.id);
    const r = await call('vlcli:tickets', { action: 'delete', id });
    if (!r.ok) {
        error.value = r.message || '删除失败';
        return;
    }
    toast('这条工单已从本地移除', 'info');
    view.value = 'list';
    await load();
}

const stopDrop = onDropPaths((paths) => {
    if (view.value === 'list') return;
    dropping.value = false;
    addPaths(paths);
});
onBeforeUnmount(() => stopDrop());

onMounted(() => void load());
</script>

<template>
    <div class="vl-card" data-ui="voxlink:tickets">
        <div class="vl-card-head">
            <div>
                <h2>我的工单{{ unread ? ` · ${unread} 条未读` : '' }}</h2>
                <p>问题和附件发给 VoxLink 那边，回复后回到这里刷新就能看到。服务端删掉的工单会自动消失。</p>
            </div>
            <div class="vl-row">
                <button v-if="view === 'list'" class="btn btn-ghost btn-sm" :disabled="busy" @click="load">刷新</button>
                <button v-if="view === 'list'" class="btn btn-sm" @click="view = 'create'">新建工单</button>
                <button v-if="view !== 'list'" class="btn btn-ghost btn-sm" @click="view = 'list'">返回列表</button>
            </div>
        </div>

        <p v-if="error" class="vl-error" role="alert">{{ error }}</p>

        <template v-if="view === 'list'">
            <div v-if="busy && !tickets.length" class="vl-empty">正在同步工单…</div>
            <div v-else-if="tickets.length" class="vl-grid">
                <article v-for="t in pageTickets" :key="str(t.id)" class="vl-room">
                    <div class="vl-room-meta">
                        <span class="vl-pill" :class="state(t).cls">{{ state(t).text }}</span>
                        <span class="vl-pill">#{{ str(t.id).slice(0, 8) }}</span>
                    </div>
                    <div class="vl-room-name">{{ str(t.description) || '（没有描述）' }}</div>
                    <div class="vl-players">{{ stamp(t.lastTimeMs || t.timeMs) }}</div>
                    <button class="btn btn-sm" @click="open(str(t.id))">查看</button>
                </article>
            </div>
            <div v-else class="vl-empty">还没有工单</div>
            <div v-if="tickets.length > PAGE" class="vl-actions">
                <button class="btn btn-ghost btn-sm" :disabled="page <= 0" @click="page -= 1">上一页</button>
                <span class="vl-hint">第 {{ page + 1 }} / {{ Math.ceil(tickets.length / PAGE) }} 页</span>
                <button class="btn btn-ghost btn-sm" :disabled="(page + 1) * PAGE >= tickets.length" @click="page += 1">
                    下一页
                </button>
            </div>
        </template>

        <template v-else>
            <template v-if="detail">
                <div class="vl-room-meta" style="margin-bottom: 10px">
                    <span class="vl-pill">#{{ str(detail.id) }}</span>
                    <span class="vl-pill">{{ stamp(detail.time, true) }}</span>
                </div>
                <p class="vl-hint">{{ str(detail.description) }}</p>
                <div v-if="attachments.length" class="vl-row" style="margin: 8px 0">
                    <span v-for="(f, i) in attachments" :key="i" class="vl-pill">
                        {{ str(f.name) }} · {{ (Number(f.size) / 1048576).toFixed(1) }} MB
                    </span>
                </div>
                <article v-for="(m, i) in msgSlice" :key="i" class="vl-notice" style="margin-top: 8px">
                    <strong>{{ str(m.from) === 'admin' ? 'VoxLink 回复' : '我的追问' }}</strong>
                    <p class="vl-hint">{{ str(m.text || m.content) }}</p>
                    <p class="vl-players">{{ stamp(m.time || m.timeMs, !m.timeMs) }}</p>
                </article>
                <div v-if="msgs.length > 5" class="vl-actions">
                    <button class="btn btn-ghost btn-sm" :disabled="msgPage <= 0" @click="msgPage -= 1">较早消息</button>
                    <button class="btn btn-ghost btn-sm" :disabled="(msgPage + 1) * 5 >= msgs.length" @click="msgPage += 1">
                        较新消息
                    </button>
                </div>
            </template>

            <label class="vl-field" style="margin-top: 14px">
                <span>{{ isCreate ? '问题描述' : '追问内容' }}</span>
                <textarea
                    v-model="text"
                    class="input"
                    rows="5"
                    :disabled="busy"
                    :placeholder="isCreate ? '发生了什么、怎么复现、什么版本' : '补充信息，或者回答追问'"
                ></textarea>
            </label>

            <div
                class="vl-empty"
                :class="{ dropping }"
                style="padding: 18px 12px; cursor: pointer"
                @click="pickFiles"
                @dragenter.prevent="dropping = true"
                @dragover.prevent="dropping = true"
                @dragleave.prevent="dropping = false"
            >
                点这里选附件，或者把文件直接拖到窗口上。一条消息最多 {{ FILES_MAX }} 个，总量不超过 500 MB。
            </div>

            <div v-if="files.length" class="vl-row" style="margin-top: 10px">
                <span v-for="(f, i) in files" :key="f" class="vl-pill ok" @click.stop="files.splice(i, 1)">
                    {{ base(f) }} <UiGlyph name="close" :size="11" />
                </span>
            </div>

            <div class="vl-actions">
                <button class="btn" :disabled="busy" @click="send">{{ isCreate ? '提交' : '发送追问' }}</button>
                <button v-if="!isCreate && detail" class="btn btn-ghost" :disabled="busy" @click="remove">删除工单</button>
            </div>
        </template>
    </div>
</template>

<style scoped>
.vl-empty.dropping {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent-2);
}
.vl-pill.ok {
    cursor: pointer;
}
</style>
