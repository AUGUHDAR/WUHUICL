<script setup lang="ts">
/**
 * 实例快照（后悔药）：列表 + 拍快照 + 回滚 + 删除。
 * 数据由后端 snapshots:* 提供；这里只负责呈现与确认，不做任何删除/覆盖的静默动作。
 */
import { computed, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../store';
import { errText } from '../api';
import ConfirmModal from './ConfirmModal.vue';
import EmptyState from './EmptyState.vue';
import UiGlyph from './UiGlyph.vue';

const props = defineProps<{
    folder: string;
    versionId: string;
    instanceName: string;
}>();

interface SnapshotMeta {
    id: string;
    createdAt: number;
    reason: string;
    auto: boolean;
    bytes: number;
    zipBytes: number;
    available: boolean;
    counts: Record<string, number>;
}

const list = ref<SnapshotMeta[]>([]);
const loading = ref(false);
const busy = ref(false);
const error = ref('');
const expanded = ref(false);
const confirmRestore = ref<SnapshotMeta | null>(null);
const confirmDelete = ref<SnapshotMeta | null>(null);

const KIND_TEXT: Record<string, string> = {
    mods: '模组',
    resourcepacks: '资源包',
    shaderpacks: '光影'
};

function fmtBytes(n: number): string {
    if (!n) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB'];
    let v = n,
        i = 0;
    while (v >= 1024 && i < units.length - 1) {
        v /= 1024;
        i++;
    }
    return `${v.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}
function fmtTime(ms: number): string {
    const d = new Date(ms);
    const p = (n: number) => String(n).padStart(2, '0');
    return `${d.getFullYear()}/${p(d.getMonth() + 1)}/${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}
function countsText(m: SnapshotMeta): string {
    return Object.entries(m.counts || {})
        .map(([k, v]) => `${KIND_TEXT[k] ?? k} ${v}`)
        .join(' · ');
}

async function load() {
    if (!props.folder || !props.versionId) return;
    loading.value = true;
    error.value = '';
    try {
        list.value = (await invoke<SnapshotMeta[]>('ipc_invoke', {
            channel: 'snapshots:list',
            args: [props.folder, props.versionId]
        })) as SnapshotMeta[];
    } catch (e) {
        error.value = errText(e);
    } finally {
        loading.value = false;
    }
}
async function take() {
    if (busy.value) return;
    busy.value = true;
    error.value = '';
    try {
        await invoke('ipc_invoke', {
            channel: 'snapshots:create',
            args: [props.folder, props.versionId, props.instanceName, '手动快照', false]
        });
        toast('快照已保存', 'success');
        await load();
    } catch (e) {
        error.value = errText(e);
        toast('创建快照失败：' + error.value, 'error');
    } finally {
        busy.value = false;
    }
}
async function doRestore() {
    const meta = confirmRestore.value;
    confirmRestore.value = null;
    if (!meta || busy.value) return;
    busy.value = true;
    try {
        const r = (await invoke('ipc_invoke', {
            channel: 'snapshots:restore',
            args: [props.folder, props.versionId, meta.id, props.instanceName]
        })) as { restored?: number; safetySnapshot?: string };
        toast(
            `已回滚到 ${fmtTime(meta.createdAt)} 的快照（还原 ${r?.restored ?? 0} 个文件）` +
                (r?.safetySnapshot ? '；回滚前的状态也已存为快照' : ''),
            'success'
        );
        await load();
    } catch (e) {
        toast('回滚失败：' + errText(e), 'error');
    } finally {
        busy.value = false;
    }
}
async function doDelete() {
    const meta = confirmDelete.value;
    confirmDelete.value = null;
    if (!meta) return;
    try {
        await invoke('ipc_invoke', {
            channel: 'snapshots:delete',
            args: [props.folder, props.versionId, meta.id]
        });
        toast('快照已删除', 'info');
        await load();
    } catch (e) {
        toast('删除失败：' + errText(e), 'error');
    }
}
load();

const totalBytes = computed(() => list.value.reduce((s, m) => s + (m.zipBytes || 0), 0));
</script>
<template>
    <div class="snap-card">
        <div class="snap-head">
            <div class="snap-title">
                <UiGlyph name="clock" :size="17" />
                <h3>快照</h3>
                <span v-if="list.length" class="snap-count">
                    {{ list.length }} 份 · {{ fmtBytes(totalBytes) }}
                </span>
            </div>
            <div class="snap-actions">
                <button class="btn btn-ghost btn-sm" :disabled="loading" @click="load">
                    {{ loading ? '读取中…' : '刷新' }}
                </button>
                <button class="btn btn-gold btn-sm" :disabled="busy" @click="take">
                    {{ busy ? '处理中…' : '拍摄快照' }}
                </button>
            </div>
        </div>
        <p class="snap-hint">
            把当前实例的模组、资源包、光影整体存成一份，需要时一键回滚到那一刻。
            回滚前会自动再存一份当前状态，回滚本身也能撤销。
        </p>
        <p v-if="error" class="snap-error" role="alert">{{ error }}</p>
        <ul v-if="list.length" class="snap-list" :class="{ expanded }">
            <li v-for="meta in list" :key="meta.id" class="snap-row">
                <div class="snap-meta">
                    <div class="snap-row-top">
                        <strong>{{ fmtTime(meta.createdAt) }}</strong>
                        <span class="snap-tag" :class="{ auto: meta.auto }">{{
                            meta.auto ? '自动' : '手动'
                        }}</span>
                        <span v-if="!meta.available" class="snap-tag gone">文件已丢失</span>
                    </div>
                    <p class="snap-reason">{{ meta.reason }}</p>
                    <p class="snap-counts">{{ countsText(meta) }}</p>
                </div>
                <div class="snap-row-actions">
                    <button
                        class="btn btn-ghost btn-sm"
                        :disabled="busy || !meta.available"
                        @click="confirmRestore = meta"
                    >
                        回滚到这份
                    </button>
                    <button
                        class="btn btn-ghost btn-sm"
                        :disabled="busy"
                        @click="confirmDelete = meta"
                    >
                        删除
                    </button>
                </div>
            </li>
        </ul>
        <!-- 空态收编 EmptyState（UX「空状态指引」）：入口按钮已在卡头（拍摄快照），无需 #action -->
        <EmptyState
            v-else-if="!loading"
            icon="clock"
            title="还没有快照"
            description="装大整合包、批量更新模组之前，建议先拍一份——出问题能一键退回。"
        />
        <button
            v-if="list.length > 4"
            class="snap-more"
            type="button"
            @click="expanded = !expanded"
        >
            {{ expanded ? '收起' : `展开全部 ${list.length} 份` }}
        </button>

        <ConfirmModal
            :open="!!confirmRestore"
            title="回滚到这个快照？"
            :message="
                confirmRestore
                    ? `将把「${instanceName}」的模组、资源包、光影整体替换为 ${fmtTime(confirmRestore.createdAt)} 的状态。当前状态会先自动存一份新快照，随时可以再切回来。`
                    : ''
            "
            confirm-text="回滚"
            :busy="busy"
            @confirm="doRestore"
            @cancel="confirmRestore = null"
        />
        <ConfirmModal
            :open="!!confirmDelete"
            title="删除这份快照？"
            :message="
                confirmDelete
                    ? `删除 ${fmtTime(confirmDelete.createdAt)} 的快照文件，不影响当前实例内容。`
                    : ''
            "
            confirm-text="删除快照"
            @confirm="doDelete"
            @cancel="confirmDelete = null"
        />
    </div>
</template>
<style scoped>
.snap-card {
    padding: var(--card-pad);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    /* 实底（模糊只在 .shell 一层，见 App.vue） */
    background: var(--surface-solid);
    display: grid;
    gap: var(--space-3);
}
.snap-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
}
.snap-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--text);
}
.snap-title h3 {
    font-size: var(--text-lg);
    margin: 0;
}
.snap-count {
    font-size: var(--text-xs);
    color: var(--text-dim);
}
.snap-actions {
    display: flex;
    gap: var(--space-2);
}
.snap-hint {
    margin: 0;
    font-size: var(--text-xs);
    line-height: 1.7;
    color: var(--text-dim);
}
.snap-error {
    margin: 0;
    color: var(--danger);
    font-size: var(--text-xs);
}
.snap-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    max-height: 300px;
    overflow-y: auto;
}
.snap-list.expanded {
    max-height: none;
}
.snap-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-3) 0;
    border-top: 1px solid color-mix(in srgb, var(--border) 55%, transparent);
    transition: background 0.15s ease;
}
/* 行 hover 统一规则【整组苏醒】（inventory L，参照 GameView .installed-row）：行背景 var(--hover)，
 * 行内操作按钮整组提亮（背景 card-2 + 边框 border-strong + 文字提亮），不做位移。 */
.snap-row:hover {
    background: var(--hover);
}
.snap-row:hover :is(.snap-row-actions .btn):not(:disabled),
.snap-row:focus-within :is(.snap-row-actions .btn):not(:disabled) {
    background: var(--card-2);
    border-color: var(--border-strong);
    color: var(--text);
}
.snap-row:first-child {
    border-top: 0;
}
.snap-row-top {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
}
.snap-row-top strong {
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
}
.snap-tag {
    font-size: 11px;
    padding: 1px 6px;
    border-radius: 5px;
    border: 1px solid var(--border);
    color: var(--text-dim);
}
.snap-tag.auto {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
    color: var(--accent-2);
}
.snap-tag.gone {
    border-color: var(--danger-border);
    color: var(--danger);
}
.snap-reason,
.snap-counts {
    margin: 3px 0 0;
    font-size: var(--text-xs);
    color: var(--text-dim);
}
.snap-row-actions {
    display: flex;
    gap: var(--space-2);
    flex: none;
}
.snap-more {
    justify-self: start;
    background: none;
    border: 0;
    padding: 0;
    font-size: var(--text-xs);
    color: var(--accent-2);
    cursor: pointer;
}
</style>
