<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import MemoryOrganizer from './MemoryOrganizer.vue';
import {
    addCustomJava,
    cancelJavaScan,
    errText,
    getSystemInfo,
    hideJava,
    listJava,
    onProgress,
    pickAddJava,
    refreshJava
} from '../../../api';
import { store, toast } from '../../../store';
import { autoMemoryMB } from '@shared/memory';
import { useSettingsPersistence } from './useSettingsPersistence';

defineProps<{ visible: boolean }>();
const { save } = useSettingsPersistence();

const javas = ref<Awaited<ReturnType<typeof listJava>>>([]);
const javaLoading = ref(true);
const javaError = ref('');
const javaRefreshing = ref(false);
const javaCancelling = ref(false);
const javaScanText = ref('');
const javaScanProgress = ref(0);
const javaAdding = ref(false);
const javaCustomInput = ref('');
const javaAddError = ref('');

async function refreshJavaList(refresh = true, announce = true) {
    if (javaRefreshing.value) {
        javaCancelling.value = true;
        try {
            const cancelled = await cancelJavaScan();
            if (!cancelled) toast('扫描任务已结束', 'info');
        } catch (error) {
            toast('取消扫描失败：' + errText(error), 'error');
        } finally {
            javaCancelling.value = false;
        }
        return;
    }
    javaRefreshing.value = true;
    javaScanText.value = '正在准备扫描全部本地固定磁盘…';
    javaScanProgress.value = 0;
    try {
        javas.value = await refreshJava(refresh);
        if (announce) toast('Java 扫描完成', 'success');
    } catch (error) {
        const message = errText(error);
        const cancelled = /取消|abort/i.test(message);
        toast(cancelled ? 'Java 扫描已取消' : '扫描失败：' + message, cancelled ? 'info' : 'error');
    } finally {
        javaRefreshing.value = false;
        javaCancelling.value = false;
    }
}

async function addJava() {
    if (javaAdding.value) return;
    javaAdding.value = true;
    javaAddError.value = '';
    try {
        const path = javaCustomInput.value.trim();
        if (path) {
            await addCustomJava(path);
            javaCustomInput.value = '';
            javas.value = await listJava();
        } else {
            const list = await pickAddJava();
            if (!list) return;
            javas.value = list;
        }
        toast('已添加 Java', 'success');
    } catch (error) {
        javaAddError.value = errText(error);
    } finally {
        javaAdding.value = false;
    }
}

async function hideJavaPath(path: string) {
    try {
        await hideJava(path);
        javas.value = await listJava();
    } catch (error) {
        toast('操作失败：' + errText(error), 'error');
    }
}

onMounted(async () => {
    try {
        javas.value = await listJava();
    } catch (error) {
        javaError.value = errText(error);
    } finally {
        javaLoading.value = false;
    }
    void refreshJavaList(false, false);
});

const stopJavaProgress = onProgress((event) => {
    if (event.stage !== 'java-scan') return;
    javaScanText.value = event.text;
    javaScanProgress.value = Math.max(0, Math.min(1, event.overall ?? event.progress));
});
onUnmounted(stopJavaProgress);

const javaLabel = (java: { major: number; path: string; version: string; architecture?: string }) =>
    `Java ${java.major}（${java.version} · ${java.architecture ?? '未知架构'}）· ${java.path}`;

const MEM_MIN = 1024;
const MEM_STEP = 512;
const SYS_RESERVE_MB = 1024;
const memMax = ref(16384);
const memTotal = ref(0);
const memFree = ref(0);
const fmtMem = (mb: number) =>
    mb % 1024 === 0 ? `${mb / 1024}G` : `${mb}MB（${(mb / 1024).toFixed(2)}G）`;

async function refreshSystemInfo(): Promise<void> {
    try {
        const info = await getSystemInfo();
        memTotal.value = info.totalMemMB;
        memFree.value = info.freeMemMB;
        memMax.value = Math.max(
            MEM_MIN,
            Math.floor((info.freeMemMB - SYS_RESERVE_MB) / MEM_STEP) * MEM_STEP
        );
        const settings = store.settings;
        if (settings && settings.memoryMB > info.totalMemMB) {
            settings.memoryMB = info.totalMemMB;
            void save({ memoryMB: info.totalMemMB });
        }
    } catch {
        /* Keep the conservative fallback when system information is unavailable. */
    }
}
onMounted(refreshSystemInfo);

const memoryAuto = computed(() => store.settings?.memoryAuto === true);
function onMemoryAutoChange(on: boolean): void {
    store.settings!.memoryAuto = on;
    void save({ memoryAuto: on });
}
const autoMemMB = computed(() => autoMemoryMB(memTotal.value || 16384));
const autoMemoryText = computed(() => (memTotal.value ? fmtMem(autoMemMB.value) : '…'));
const memoryOverFree = computed(() => {
    const mb = store.settings?.memoryMB ?? 0;
    return memFree.value > 0 && mb > memFree.value;
});
const memoryText = computed(() => {
    if (memoryAuto.value) return `自动（${fmtMem(autoMemMB.value)}）`;
    if (memPreview.value != null)
        return fmtMem(Math.round(memPreview.value / MEM_STEP) * MEM_STEP);
    return fmtMem(store.settings?.memoryMB ?? 0);
});
const memoryInfoText = computed(() =>
    memTotal.value
        ? `已用 ${fmtMem(Math.max(0, memTotal.value - memFree.value))} · 可用 ${fmtMem(memFree.value)} · 总计 ${fmtMem(memTotal.value)}`
        : '正在读取本机内存信息…'
);

const memTrack = ref<HTMLElement | null>(null);
const memDragging = ref(false);
const memPreview = ref<number | null>(null);
const memBaseline = ref<{ max: number; span: number } | null>(null);
const memScaleMax = computed(() => memBaseline.value?.max ?? memMax.value);
const memScaleSpan = computed(
    () => memBaseline.value?.span ?? Math.max(memMax.value, MEM_MIN + MEM_STEP) - MEM_MIN
);
function memRawFromClientX(clientX: number): number {
    const track = memTrack.value;
    if (!track) return store.settings?.memoryMB ?? MEM_MIN;
    const rect = track.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    return MEM_MIN + ratio * memScaleSpan.value;
}
function onMemThumbDown(event: PointerEvent) {
    event.preventDefault();
    event.stopPropagation();
    memDragging.value = true;
    const max = memMax.value;
    memBaseline.value = { max, span: Math.max(max, MEM_MIN + MEM_STEP) - MEM_MIN };
    memPreview.value = store.settings?.memoryMB ?? MEM_MIN;
    (event.target as HTMLElement).setPointerCapture(event.pointerId);
}
function onMemPointerMove(event: PointerEvent) {
    if (!memDragging.value) return;
    memPreview.value = memRawFromClientX(event.clientX);
}
function onMemPointerUp() {
    if (!memDragging.value) return;
    memDragging.value = false;
    const raw = memPreview.value ?? store.settings?.memoryMB ?? MEM_MIN;
    const frozenMax = memBaseline.value?.max ?? memMax.value;
    memPreview.value = null;
    memBaseline.value = null;
    const value = Math.max(
        MEM_MIN,
        Math.min(Math.round(raw / MEM_STEP) * MEM_STEP, frozenMax)
    );
    if (store.settings) {
        store.settings.memoryMB = value;
        void save({ memoryMB: value });
    }
    void refreshSystemInfo();
}

const memoryInputGB = ref('');
const memoryEditing = ref(false);
const vFocus = { mounted: (element: HTMLElement) => element.focus() };
function startMemoryEdit() {
    memoryInputGB.value = String(((store.settings?.memoryMB ?? MEM_MIN) / 1024).toFixed(2)).replace(
        /\.?0+$/,
        ''
    );
    memoryEditing.value = true;
}
function commitMemoryEdit() {
    const gb = Number(memoryInputGB.value);
    memoryEditing.value = false;
    if (!Number.isFinite(gb) || gb <= 0) return;
    const mb = Math.round(Math.max(MEM_MIN / 1024, Math.min(gb, memMax.value / 1024)) * 1024);
    if (store.settings) {
        store.settings.memoryMB = mb;
        void save({ memoryMB: mb });
    }
}
const memFillPct = computed(() => {
    const mb = memPreview.value ?? store.settings?.memoryMB ?? MEM_MIN;
    return Math.max(0, Math.min(100, ((mb - MEM_MIN) / memScaleSpan.value) * 100));
});
</script>

<template>
    <div v-show="visible" data-ui="SettingsView:49e12a3919db" class="runtime-grid">
        <details
            data-ui="SettingsView:af57e3969b2b"
            class="card group collapse setting-target"
            data-section="memory"
            tabindex="-1"
            open
        >
            <summary data-ui="SettingsView:3d85eb6ecd9d" class="collapse-head">
                <h3 data-ui="SettingsView:9a7e967e2291" class="group-title">内存分配</h3>
                <span
                    data-ui="SettingsView:1d0778d1c7b8"
                    class="collapse-arrow"
                    aria-hidden="true"
                ></span>
            </summary>
            <div data-ui="SettingsView:e23da790569f" class="collapse-body">
                <div data-ui="SettingsView:c72d10bfcf7d" class="memory-auto-row">
                    <span data-ui="SettingsView:e5cbfde2bea0" class="java-auto-text">
                        <span data-ui="SettingsView:5f69d2d3754e" class="java-auto-title"
                            >自动分配（推荐）</span
                        >
                        <span data-ui="SettingsView:8e3734942c8b" class="muted java-auto-desc"
                            >按物理内存 1/4 自动分配（本机当前
                            {{ autoMemoryText }}，2-8GB
                            区间），启动时实时生效；开启后禁用手动调节。</span
                        >
                    </span>
                    <label data-ui="SettingsView:5afa3ec5d468" class="switch">
                        <input
                            data-ui="SettingsView:acfa8cbaf083"
                            :checked="memoryAuto"
                            type="checkbox"
                            @change="
                                onMemoryAutoChange(
                                    ($event.target as HTMLInputElement).checked
                                )
                            "
                        />
                        <span data-ui="SettingsView:2a9267588521" class="switch-ui"></span>
                    </label>
                </div>
                <template v-if="!memoryAuto">
                    <div data-ui="SettingsView:cbb53c3f809e" class="memory-row">
                        <div
                            data-ui="SettingsView:2a2d1473539f"
                            ref="memTrack"
                            class="mem-slider"
                            @pointermove="onMemPointerMove"
                            @pointerup="onMemPointerUp"
                            @pointercancel="onMemPointerUp"
                        >
                            <div data-ui="SettingsView:e74d81b4804a" class="mem-slider-track"></div>
                            <div
                                data-ui="SettingsView:91a09abfe5b0"
                                class="mem-slider-fill"
                                :style="{ width: memFillPct + '%' }"
                            ></div>
                            <div
                                data-ui="SettingsView:9f6291a873d4"
                                class="mem-slider-thumb"
                                :class="{ dragging: memDragging }"
                                :style="{ left: memFillPct + '%' }"
                                @pointerdown="onMemThumbDown"
                                @pointermove="onMemPointerMove"
                                @pointerup="onMemPointerUp"
                                @pointercancel="onMemPointerUp"
                            ></div>
                        </div>
                        <input
                            data-ui="SettingsView:ff586c53928a"
                            v-if="memoryEditing"
                            v-model="memoryInputGB"
                            class="input memory-input"
                            type="number"
                            :min="MEM_MIN / 1024"
                            :max="memMax / 1024"
                            step="0.25"
                            @keydown.enter="commitMemoryEdit"
                            @keydown.esc="memoryEditing = false"
                            @blur="commitMemoryEdit"
                            v-focus
                        />
                        <span
                            data-ui="SettingsView:5c63bc2aa86a"
                            v-else
                            class="memory-value"
                            title="点击精确输入（GB）"
                            @click="startMemoryEdit"
                            >{{ memoryText }}</span
                        >
                    </div>
                    <p data-ui="SettingsView:c7ad9d43c06c" class="muted group-hint">
                        拖动滑块以 0.5 GB
                        步进（上限随当前可用内存浮动，预留 1G
                        给系统）；需要精细调节（如 0.25 GB）时点右侧数值直接输入
                    </p>
                    <p
                        data-ui="SettingsView:9377cfb85e03"
                        v-if="memoryOverFree"
                        class="memory-warn"
                    >
                        当前分配超过可用内存，游戏可能启动失败或卡死系统；请调低或改回「自动分配」
                    </p>
                </template>
                <p data-ui="SettingsView:8cff89231775" class="muted memory-info">
                    {{ memoryInfoText }}
                    <button
                        data-ui="SettingsView:87a00792aa4a"
                        class="memory-refresh"
                        type="button"
                        @click="refreshSystemInfo"
                    >
                        刷新
                    </button>
                </p>
                <MemoryOrganizer @refresh="refreshSystemInfo" />
            </div>
        </details>

        <details
            data-ui="SettingsView:b128c0a66c1b"
            class="card group collapse setting-target"
            data-section="java"
            tabindex="-1"
            open
        >
            <summary data-ui="SettingsView:a4c3bf3540ee" class="collapse-head">
                <h3 data-ui="SettingsView:71ef8a822bf4" class="group-title">Java 运行时</h3>
                <span
                    data-ui="SettingsView:556ae4c0b4a4"
                    class="collapse-arrow"
                    aria-hidden="true"
                ></span>
            </summary>
            <div data-ui="SettingsView:2680ccfc46ae" class="collapse-body">
                <label data-ui="SettingsView:2937d97802dc" class="java-auto-row">
                    <span data-ui="SettingsView:0d87aa299341" class="java-auto-text">
                        <span data-ui="SettingsView:c6677e7d47e3" class="java-auto-title"
                            >自动检测并下载所需 Java（推荐）</span
                        >
                        <span data-ui="SettingsView:9299ef0ab8d3" class="muted java-auto-desc">
                            启动时按游戏版本自动选择匹配的
                            Java；本机没有时自动下载安装。关闭后使用下方手动选择的
                            Java。
                        </span>
                    </span>
                    <span data-ui="SettingsView:6c7dc29133fc" class="switch">
                        <input
                            data-ui="SettingsView:84de310dd205"
                            type="checkbox"
                            :checked="store.settings!.javaAuto"
                            @change="
                                save({
                                    javaAuto: ($event.target as HTMLInputElement).checked
                                })
                            "
                        />
                        <span data-ui="SettingsView:3a5d5cac5431" class="switch-ui"></span>
                    </span>
                </label>
                <div
                    data-ui="SettingsView:033fa130c6ea"
                    v-if="javaLoading"
                    class="java-loading"
                >
                    <span data-ui="SettingsView:9d3e8d9207ae" class="spin"></span>
                    <span data-ui="SettingsView:f482492d6fc3" class="muted"
                        >正在检测本机 Java…</span
                    >
                </div>
                <template v-else>
                    <select
                        data-ui="SettingsView:47dcfd6bb554"
                        v-model="store.settings!.javaPath"
                        class="select"
                        :disabled="store.settings!.javaAuto"
                        @change="save({ javaPath: store.settings!.javaPath })"
                    >
                        <option value="">自动选择（推荐）</option>
                        <option v-for="java in javas" :key="java.path" :value="java.path">
                            {{ javaLabel(java) }}
                        </option>
                    </select>
                    <p data-ui="SettingsView:a4338b6dbdab" v-if="javaError" class="group-error">
                        Java 检测失败：{{ javaError }}
                    </p>
                    <p
                        data-ui="SettingsView:b8220f037cc7"
                        v-else-if="!javas.length"
                        class="muted group-hint"
                    >
                        未检测到本机 Java，将使用「自动选择」或在启动时自动下载。
                    </p>
                    <details data-ui="SettingsView:4a4a9ec6663a" class="java-detected">
                        <summary data-ui="SettingsView:e23d6518ed47">
                            本机 Java 与手动管理（{{ javas.length }}）
                        </summary>
                        <div data-ui="SettingsView:205e9439c186" class="java-scan-row">
                            <div data-ui="SettingsView:b5bbe482ca7a" class="java-scan-status">
                                <span data-ui="SettingsView:3e2d50286c09" class="muted"
                                    >扫描注册表、PATH、启动器 Runtime
                                    与全部本地固定磁盘</span
                                >
                                <span
                                    data-ui="SettingsView:95cdba118743"
                                    v-if="javaRefreshing"
                                    class="muted java-scan-text"
                                    :title="javaScanText"
                                >
                                    {{ javaScanText }}
                                </span>
                                <div
                                    data-ui="SettingsView:8b0c0b54c54f"
                                    v-if="javaRefreshing"
                                    class="java-scan-track"
                                    aria-label="Java 扫描进度"
                                >
                                    <span
                                        data-ui="SettingsView:ac6550af67ec"
                                        :style="{ width: `${javaScanProgress * 100}%` }"
                                    ></span>
                                </div>
                            </div>
                            <button
                                data-ui="SettingsView:9df6cab1a2e9"
                                class="btn btn-ghost btn-sm"
                                :disabled="javaCancelling"
                                @click="refreshJavaList()"
                            >
                                {{
                                    javaCancelling
                                        ? '正在取消…'
                                        : javaRefreshing
                                          ? '取消扫描'
                                          : '重新扫描'
                                }}
                            </button>
                        </div>
                        <div data-ui="SettingsView:6dc6e26726aa" v-if="javas.length" class="java-list">
                            <div data-ui="SettingsView:6bd22de0fe44" class="java-list-head">
                                <span data-ui="SettingsView:bcca822500ec" class="muted"
                                    >已识别 {{ javas.length }} 个 Java</span
                                >
                            </div>
                            <div
                                data-ui="SettingsView:6cc4e456c280"
                                v-for="java in javas"
                                :key="java.path"
                                class="java-item"
                            >
                                <span
                                    data-ui="SettingsView:b1a116a1a7bb"
                                    class="tag"
                                    :class="java.source === 'manual' ? 'tag-accent' : ''"
                                >
                                    {{ java.source === 'manual' ? '手动' : '自动' }}
                                </span>
                                <span data-ui="SettingsView:5131692582d3" class="java-item-ver"
                                    >Java {{ java.major }}</span
                                >
                                <span
                                    data-ui="SettingsView:7972354cc877"
                                    class="muted java-item-path"
                                    :title="`${java.vendor ?? '未知发行版'} · ${java.architecture ?? (java.is64Bit ? '64 位' : '32 位')} · ${java.sourceDetail ?? ''}\n${java.path}`"
                                >
                                    {{ java.vendor ?? 'Java' }} ·
                                    {{ java.architecture ?? (java.is64Bit ? '64 位' : '32 位') }}
                                    · {{ java.path }}
                                </span>
                                <button
                                    data-ui="SettingsView:0360c51d1055"
                                    class="java-item-hide"
                                    title="从列表隐藏"
                                    @click="hideJavaPath(java.path)"
                                >
                                    ×
                                </button>
                            </div>
                        </div>
                        <div data-ui="SettingsView:47c462c48d2f" class="java-add-row">
                            <input
                                data-ui="SettingsView:03420f9eb5b5"
                                v-model="javaCustomInput"
                                class="input mono"
                                placeholder="粘贴 java 可执行文件完整路径回车添加，或留空点「添加」选择文件…"
                                spellcheck="false"
                                @keyup.enter="addJava"
                            />
                            <button
                                data-ui="SettingsView:85975bff1446"
                                class="btn btn-ghost"
                                :disabled="javaAdding"
                                @click="addJava"
                            >
                                {{ javaAdding ? '校验中…' : '添加' }}
                            </button>
                        </div>
                        <p
                            data-ui="SettingsView:e6688869ed97"
                            v-if="javaAddError"
                            class="group-error"
                        >
                            {{ javaAddError }}
                        </p>
                    </details>
                </template>
            </div>
        </details>

        <details data-ui="SettingsView:612d9b820913" class="card group" data-section="jvm">
            <summary data-ui="SettingsView:3cc49dcec409" class="group-title">
                高级 · JVM 参数
            </summary>
            <input
                data-ui="SettingsView:cce61f7d8c4e"
                v-model="store.settings!.jvmArgs"
                class="input mono"
                placeholder="例如：-XX:+UseG1GC -XX:+ParallelRefProcEnabled"
                @change="save({ jvmArgs: store.settings!.jvmArgs })"
            />
            <p data-ui="SettingsView:1619a3a7af4e" class="muted group-hint">
                高级选项，留空则使用默认参数
            </p>
        </details>
    </div>
</template>

<style scoped>
.runtime-grid {
    display: grid;
    gap: 12px;
}
.java-auto-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
}
.java-auto-text {
    display: flex;
    flex-direction: column;
    gap: 4px;
}
.java-auto-title {
    font-size: 13px;
    font-weight: 600;
}
.java-auto-desc {
    font-size: 12px;
    line-height: 1.6;
}
.java-scan-row,
.java-add-row,
.memory-auto-row,
.memory-row {
    display: flex;
    align-items: center;
    gap: 10px;
}
.java-scan-status {
    display: flex;
    flex: 1;
    min-width: 0;
    flex-direction: column;
    gap: 5px;
}
.java-scan-text {
    overflow: hidden;
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.java-scan-track {
    height: 3px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--border);
}
.java-scan-track span {
    display: block;
    height: 100%;
    background: var(--accent);
}
.java-list {
    display: grid;
    gap: 4px;
}
.java-item {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 7px 0;
    border-bottom: 1px solid var(--border);
}
.java-item-ver {
    flex: none;
    font-size: 12px;
}
.java-item-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.java-item-hide {
    border: 0;
    background: transparent;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 18px;
}
.java-add-row .input {
    flex: 1;
}
.java-loading {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 0;
}
.memory-row {
    min-width: 0;
}
.mem-slider {
    position: relative;
    flex: 1;
    height: 20px;
    cursor: default;
    touch-action: none;
}
.mem-slider-track,
.mem-slider-fill {
    position: absolute;
    top: 8px;
    right: 0;
    left: 0;
    height: 4px;
    border-radius: 2px;
}
.mem-slider-track {
    background: var(--border);
}
.mem-slider-fill {
    right: auto;
    background: var(--accent);
}
.mem-slider-thumb {
    position: absolute;
    top: 3px;
    width: 14px;
    height: 14px;
    margin-left: -7px;
    border: 2px solid var(--accent);
    border-radius: 50%;
    background: var(--card-solid, var(--card));
    cursor: grab;
}
.mem-slider-thumb.dragging {
    cursor: grabbing;
}
.memory-value {
    flex: none;
    min-width: 88px;
    font-size: 12px;
    text-align: right;
    cursor: text;
}
.memory-input {
    width: 96px;
}
.memory-warn {
    color: var(--danger);
    font-size: 12px;
    line-height: 1.6;
}
.memory-info {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    font-size: 11px;
}
.memory-refresh {
    border: 0;
    background: transparent;
    color: var(--accent);
    cursor: pointer;
    font: inherit;
}
@media (max-width: 700px) {
    .java-scan-row,
    .java-add-row {
        align-items: stretch;
        flex-direction: column;
    }
}
</style>
