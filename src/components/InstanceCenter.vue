<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { IPC, type JavaInfo, type WorldImportInfo } from '@shared/types';
import type {
    BackupManifest,
    DiagnosticFinding,
    InstanceOperation,
    InstanceOverview,
    InstanceScreenshot,
    InstanceWorld
} from '@shared/instanceCenter';
import type { ModChangePlan } from '@shared/modManagement';
import UiGlyph from './UiGlyph.vue';
import { instanceCenter } from '../instanceCenter';
import { managedImageUrl } from '../managedAssets';
import { store, refreshInstalled, toast, openSettings } from '../store';
import {
    errText,
    setVersionJava,
    exportLaunchLogs,
    selectFile,
    probeWorld,
    importWorld,
    exportMrpack,
    exportCurseforge
} from '../api';
import SelectMenu from './SelectMenu.vue';
const previousFocus = document.activeElement as HTMLElement | null;
const target = structuredClone({ ...instanceCenter.target! });
const tab = ref(instanceCenter.tab),
    busy = ref(false),
    loading = ref(true),
    error = ref(''),
    overview = ref<InstanceOverview>();
const worlds = ref<InstanceWorld[]>([]),
    backups = ref<BackupManifest[]>([]),
    shots = ref<InstanceScreenshot[]>([]),
    shotTotal = ref(0),
    page = ref(0),
    search = ref(''),
    lightbox = ref(-1);
const findings = ref<DiagnosticFinding[]>([]),
    java = ref<JavaInfo[]>([]),
    javaPath = ref(''),
    planId = ref(''),
    session = ref('');
const form = ref<'' | 'clone' | 'restore' | 'import' | 'disable' | 'dependencies'>(''),
    name = ref(''),
    destination = ref(target.folder),
    includeSaves = ref(true),
    includeShots = ref(false),
    restore = ref<BackupManifest>(),
    overwrite = ref(false);
const dependencies = ref<ModChangePlan>();
const selectedMods = ref<string[]>([]);
const worldInput = ref(''),
    worldInfo = ref<WorldImportInfo>(),
    candidate = ref(''),
    mismatch = ref(false);
const sections = [
    ['overview', '概览'],
    ['worlds', '存档'],
    ['screenshots', '截图'],
    ['backups', '备份'],
    ['diagnostics', '诊断']
];
const folders = computed(() =>
    (store.settings?.folders || []).map((f) => ({ value: f.path, label: f.name }))
);
const filtered = computed(() =>
    worlds.value.filter((w) => w.name.toLowerCase().includes(search.value.toLowerCase()))
);
const image = computed(() => shots.value[lightbox.value]);
const fullImage = ref('');
watch(image, async (selected) => {
    fullImage.value = '';
    if (!selected) return;
    try {
        const data = await invoke<string>(IPC.centerFile, target, 'preview', selected.id);
        if (image.value?.id === selected.id) fullImage.value = managedImageUrl(data);
    } catch (e) {
        error.value = errText(e);
    }
});
const invoke = <T,>(channel: string, ...args: unknown[]) =>
    window.kamucl.invoke(channel, ...args) as Promise<T>;
const formatDate = (v: string | number | undefined) => (v ? new Date(v).toLocaleString() : '未知');
const size = (bytes: number) =>
    bytes >= 1024 ** 3
        ? (bytes / 1024 ** 3).toFixed(1) + ' GB'
        : (bytes / 1024 ** 2).toFixed(1) + ' MB';
async function refresh() {
    loading.value = true;
    error.value = '';
    try {
        overview.value = await invoke(IPC.centerOverview, target);
        if (tab.value === 'worlds')
            worlds.value = (await invoke<InstanceWorld[]>(IPC.centerWorlds, target)).map((w) =>
                w.icon ? { ...w, icon: managedImageUrl(w.icon) } : w
            );
        if (tab.value === 'backups') backups.value = await invoke(IPC.centerBackups, target);
        if (tab.value === 'screenshots') {
            const r = await invoke<{ total: number; items: InstanceScreenshot[] }>(
                IPC.centerScreenshots,
                target,
                page.value
            );
            shots.value = r.items.map((s) =>
                s.image ? { ...s, image: managedImageUrl(s.image) } : s
            );
            shotTotal.value = r.total;
        }
    } catch (e) {
        error.value = errText(e);
    } finally {
        loading.value = false;
    }
}
async function chooseTab(value: string) {
    tab.value = value;
    form.value = '';
    await refresh();
}
/** 实例备注编辑 */
const noteEditing = ref(false);
const noteDraft = ref('');
const noteSaving = ref(false);
function startNoteEdit() {
    noteDraft.value = overview.value?.note || '';
    noteEditing.value = true;
}
async function saveNote() {
    if (noteSaving.value) return;
    noteSaving.value = true;
    try {
        await perform(() => invoke(IPC.centerSetNote, target, noteDraft.value));
        noteEditing.value = false;
    } finally {
        noteSaving.value = false;
    }
}

async function perform(action: () => Promise<unknown>) {
    busy.value = true;
    error.value = '';
    try {
        await action();
        toast('操作完成', 'success');
        form.value = '';
        await refreshInstalled();
        await refresh();
    } catch (e) {
        error.value = errText(e);
    } finally {
        busy.value = false;
    }
}
const op = (operation: Omit<InstanceOperation, 'target'> & { planId?: string }) =>
    invoke(IPC.centerOperation, { ...operation, target });

/** 导出进行中的格式（'' = 空闲）。导出耗时随包体增长，用独立状态避免和 busy 互相阻塞 */
const exporting = ref<'' | 'mrpack' | 'curseforge'>('');
/** 二分会话状态（idle 时 status 为空串） */
const bisect = ref<{
    status: '' | 'testing' | 'found';
    candidates: string[];
    testing: string[];
    excluded: string[];
    round: number;
    culprit: string | null;
    contradiction: string | null;
}>({
    status: '',
    candidates: [],
    testing: [],
    excluded: [],
    round: 0,
    culprit: null,
    contradiction: null
});
const bisectBusy = ref(false);

/** 二分定位：start / report / finish / cancel 四个动作共用一条通道 */
async function bisectAct(action: string, payload: Record<string, unknown> = {}, quiet = false) {
    if (bisectBusy.value) return;
    bisectBusy.value = true;
    error.value = '';
    try {
        const r = await invoke<Record<string, unknown>>(
            'bisect:action',
            action,
            target.id,
            target.folder,
            payload
        );
        if (r && typeof r === 'object' && 'status' in r) {
            bisect.value = {
                status: (r.status as 'testing' | 'found') ?? '',
                candidates: (r.candidates as string[]) ?? [],
                testing: (r.testing as string[]) ?? [],
                excluded: (r.excluded as string[]) ?? [],
                round: (r.round as number) ?? 0,
                culprit: (r.culprit as string | null) ?? null,
                contradiction: (r.contradiction as string | null) ?? null
            };
            if (r.status === 'idle') bisect.value.status = '';
            // 后端统一给了「下一步做什么」，直接用，不在这里重复拼文案
            if (r.next && !quiet) toast(String(r.next), 'info');
        } else {
            // finish / cancel 返回 { ok }
            bisect.value = {
                status: '',
                candidates: [],
                testing: [],
                excluded: [],
                round: 0,
                culprit: null,
                contradiction: null
            };
            if (r?.culprit && !quiet) toast(`已禁用 ${r.culprit}`, 'success');
        }
        await refresh();
    } catch (e) {
        error.value = errText(e);
    } finally {
        bisectBusy.value = false;
    }
}
/**
 * 导出为整合包。成功后直接把落盘路径告诉用户（打开所在目录才是下一步动作），
 * 失败按「原因 + 怎么处理」两段说清，不只甩一句失败。
 */
async function exportPack(format: 'mrpack' | 'curseforge') {
    if (exporting.value) return;
    exporting.value = format;
    error.value = '';
    try {
        const call = format === 'mrpack' ? exportMrpack : exportCurseforge;
        const r = await call(target.id, target.folder, { name: target.id });
        const listed = r.listed;
        const extra = r.overrides;
        toast(
            `已导出：${r.path}（${listed} 个模组写进清单${extra ? `，${extra} 个文件随包附带` : ''}）`,
            'success'
        );
    } catch (e) {
        error.value = `导出失败：${errText(e)}。可先确认实例没在运行，再重试一次。`;
    } finally {
        exporting.value = '';
    }
}
function navigate(view: 'mods' | 'packs' | 'shaders' | 'game') {
    instanceCenter.target = null;
    store.currentView = view;
}
/** 跳到世界地图页并预选该存档（地图页挂载时消费 worldMapRequest） */
function openWorldMap(worldId: string) {
    store.worldMapRequest = { instanceId: target.id, world: worldId };
    store.currentView = 'worldmap';
}
function close() {
    instanceCenter.target = null;
}
function key(e: KeyboardEvent) {
    if (document.querySelector('.select-menu-float')) return;
    if (e.key === 'Tab') {
        const root =
            document.querySelector('.ic-lightbox') ||
            document.querySelector('.ic-dialog') ||
            document.querySelector('.ic');
        const elements = [
            ...(root?.querySelectorAll<HTMLElement>(
                'button:not(:disabled),input:not(:disabled),[tabindex="0"]'
            ) || [])
        ].filter((n) => n.getClientRects().length);
        if (elements.length) {
            const index = elements.indexOf(document.activeElement as HTMLElement);
            if (
                (e.shiftKey && index <= 0) ||
                (!e.shiftKey && (index === elements.length - 1 || index === -1))
            ) {
                e.preventDefault();
                elements[e.shiftKey ? elements.length - 1 : 0].focus();
            }
        }
    }
    if (e.key === 'Escape') {
        e.stopImmediatePropagation();
        if (lightbox.value >= 0) lightbox.value = -1;
        else if (form.value) form.value = '';
        else close();
    }
    if (lightbox.value >= 0 && e.key === 'ArrowRight')
        lightbox.value = Math.min(shots.value.length - 1, lightbox.value + 1);
    if (lightbox.value >= 0 && e.key === 'ArrowLeft')
        lightbox.value = Math.max(0, lightbox.value - 1);
}
async function diagnose() {
    busy.value = true;
    error.value = '';
    try {
        const r = await invoke<{
            id: string;
            findings: DiagnosticFinding[];
            java: JavaInfo[];
            session?: string;
        }>(IPC.centerDiagnose, target);
        findings.value = r.findings;
        java.value = r.java;
        javaPath.value = r.java[0]?.path || '';
        planId.value = r.id;
        session.value = r.session || '';
    } catch (e) {
        error.value = errText(e);
    } finally {
        busy.value = false;
    }
}
function openRestore(b: BackupManifest) {
    restore.value = b;
    name.value = (b.metadata?.type === 'world' ? String(b.metadata.world) : target.id) + '-恢复';
    overwrite.value = false;
    form.value = 'restore';
}
async function prepareImport() {
    try {
        const f = await selectFile();
        if (!f) return;
        const info = await probeWorld(f);
        if (!info?.candidates.length) throw new Error('未发现有效存档');
        worldInput.value = f;
        worldInfo.value = info;
        candidate.value = info.candidates[0].id;
        name.value = info.candidates[0].worldName;
        form.value = 'import';
    } catch (e) {
        error.value = errText(e);
    }
}
async function disableSelected() {
    const results = await invoke<Array<{ ok: boolean; fileName: string; error?: string }>>(
        'mods:setEnabled',
        target.id,
        target.folder,
        [...selectedMods.value],
        false
    );
    selectedMods.value = results.filter((r) => !r.ok).map((r) => r.fileName);
    if (selectedMods.value.length)
        throw new Error(
            results
                .filter((r) => !r.ok)
                .map((r) => r.fileName + '：' + r.error)
                .join('\n')
        );
    await diagnose();
}
async function checkDependencies(fileName: string) {
    busy.value = true;
    error.value = '';
    try {
        dependencies.value = await invoke('center:dependencyPlan', target, fileName);
        form.value = 'dependencies';
    } catch (e) {
        error.value = errText(e);
    } finally {
        busy.value = false;
    }
}
onMounted(async () => {
    document.addEventListener('keydown', key, true);
    await refresh();
    document.querySelector<HTMLElement>('[aria-label="关闭实例管理"]')?.focus();
    if (tab.value === 'diagnostics') {
        await diagnose();
        // 续做上次没做完的二分（会话在实例目录里，重启启动器也不丢）；静默读，不弹提示
        void bisectAct('status', {}, true);
    }
});
onUnmounted(() => {
    document.removeEventListener('keydown', key, true);
    previousFocus?.focus();
});
</script>
<template>
    <Teleport to="body">
        <div class="ic-mask" @pointerdown.self="close">
            <section
                class="ic"
                role="dialog"
                aria-modal="true"
                aria-labelledby="ic-title"
                data-ui="instance-center:page"
            >
                <header class="ic-header">
                    <div>
                        <small>实例管理中心</small>
                        <h2 id="ic-title">{{ overview?.name || target.id }}</h2>
                        <p>
                            {{ overview?.mcVersion }}
                            <span v-if="overview?.loader">· {{ overview.loader }}</span> ·
                            {{ overview?.shared ? '共享游戏目录' : '独立游戏目录' }}
                        </p>
                    </div>
                    <button class="btn btn-ghost" aria-label="关闭实例管理" @click="close">
                        <UiGlyph name="close" />
                    </button>
                </header>
                <nav class="ic-tabs" aria-label="实例功能">
                    <button
                        v-for="[value, label] in sections"
                        :key="value"
                        :class="{ active: tab === value }"
                        @click="chooseTab(value)"
                    >
                        {{ label }}
                    </button>
                </nav>
                <div :key="tab" class="ic-content">
                    <p v-if="error" class="ic-error" role="alert">
                        {{ error }}
                        <button class="btn btn-sm" :disabled="busy" @click="refresh">
                            重新加载
                        </button>
                    </p>
                    <p v-if="overview?.running" class="ic-hint">
                        此目录的游戏正在运行。请退出游戏后再复制、备份或恢复。
                    </p>
                    <p v-if="loading" class="ic-empty">正在读取实例…</p>
                    <template v-else-if="tab === 'overview'">
                        <div class="ic-summary" data-ui="instance-center:overview">
                            <div>
                                <h3>你的游戏，独立管理</h3>
                                <p class="ic-path">{{ overview?.directory }}</p>
                                <p
                                    v-if="!noteEditing"
                                    class="ic-note"
                                    :title="overview?.note ? '点击编辑备注' : '点击添加备注'"
                                    @click="startNoteEdit"
                                >
                                    <template v-if="overview?.note">{{ overview.note }}</template>
                                    <template v-else>
                                        <em>点击添加备注（给这个实例写点什么…）</em>
                                    </template>
                                </p>
                                <div v-else class="ic-note-edit">
                                    <textarea
                                        v-model="noteDraft"
                                        rows="2"
                                        maxlength="500"
                                        placeholder="给这个实例写点什么（≤500 字，仅自己可见）"
                                    ></textarea>
                                    <button
                                        class="btn btn-ghost"
                                        :disabled="noteSaving"
                                        @click="noteEditing = false"
                                    >
                                        取消</button
                                    ><button
                                        class="btn btn-gold"
                                        :disabled="noteSaving"
                                        @click="saveNote"
                                    >
                                        {{ noteSaving ? '保存中…' : '保存备注' }}
                                    </button>
                                </div>
                            </div>
                            <button
                                class="btn btn-ghost"
                                @click="
                                    perform(() => invoke(IPC.centerFile, target, 'directory', ''))
                                "
                            >
                                打开目录
                            </button>
                        </div>
                        <div class="ic-grid">
                            <button class="ic-card" @click="navigate('mods')">
                                <strong>模组管理 →</strong><span>版本、启停与兼容性</span></button
                            ><button class="ic-card" @click="navigate('packs')">
                                <strong>资源包 →</strong><span>管理材质与界面资源</span></button
                            ><button class="ic-card" @click="navigate('shaders')">
                                <strong>光影包 →</strong><span>管理游戏光影</span></button
                            ><button class="ic-card" @click="navigate('game')">
                                <strong>版本设置 →</strong><span>Java、窗口与隔离设置</span>
                            </button>
                        </div>
                        <div class="ic-card">
                            <h3>复制一个独立实例</h3>
                            <p>
                                保留模组、配置与锁定状态，原实例保持完整。可在副本里尝试新的模组组合。
                            </p>
                            <button
                                class="btn btn-gold"
                                :disabled="busy || overview?.running"
                                @click="
                                    name = target.id + '-副本';
                                    form = 'clone';
                                "
                            >
                                复制实例
                            </button>
                        </div>
                        <!-- 导出为整合包：给别人用 Prism/PCL/官方启动器导入（与「复制实例」同属整实例操作） -->
                        <div class="ic-card">
                            <h3>导出为整合包</h3>
                            <p>
                                打包成 Modrinth（.mrpack）或 CurseForge 格式，别人拿到就能导入。
                                社区认得的模组写进清单（对方启动器自动重下），认不得的随包附带，
                                不会缺文件。
                            </p>
                            <div class="ic-export-actions">
                                <!-- 两种格式等价（都能打包整个实例），同级配色，不拿主按钮暗示偏好 -->
                                <button
                                    class="btn btn-ghost"
                                    data-ui="instance-center:export-mrpack"
                                    :disabled="busy || overview?.running || exporting !== ''"
                                    @click="exportPack('mrpack')"
                                >
                                    {{ exporting === 'mrpack' ? '导出中…' : '导出 Modrinth 包' }}
                                </button>
                                <button
                                    class="btn btn-ghost"
                                    data-ui="instance-center:export-curseforge"
                                    :disabled="busy || overview?.running || exporting !== ''"
                                    @click="exportPack('curseforge')"
                                >
                                    {{
                                        exporting === 'curseforge'
                                            ? '导出中…'
                                            : '导出 CurseForge 包'
                                    }}
                                </button>
                            </div>
                        </div>
                    </template>
                    <template v-else-if="tab === 'worlds'">
                        <div class="ic-tools">
                            <input
                                v-model="search"
                                class="input"
                                placeholder="搜索存档名称"
                                aria-label="搜索存档"
                            /><button
                                class="btn btn-gold"
                                :disabled="busy || overview?.running"
                                @click="prepareImport"
                            >
                                导入存档
                            </button>
                        </div>
                        <div v-for="w in filtered" :key="w.id" class="ic-row">
                            <img v-if="w.icon" :src="w.icon" alt="" />
                            <div v-else class="ic-world-icon">▧</div>
                            <div class="ic-grow">
                                <strong>{{ w.name }}</strong
                                ><small
                                    >{{ w.version || '版本未知' }} · {{ w.mode || '模式未知' }} ·
                                    {{ formatDate(w.lastPlayed) }}</small
                                >
                                <p v-if="w.error" class="ic-error">{{ w.error }}</p>
                            </div>
                            <div class="ic-actions">
                                <button
                                    class="btn btn-sm"
                                    data-ui="instance-center:world-map"
                                    :disabled="!!w.error"
                                    title="在世界地图页查看该存档的实际地形"
                                    @click="openWorldMap(w.id)"
                                >
                                    地图</button
                                ><button
                                    class="btn btn-sm"
                                    @click="
                                        perform(() => invoke(IPC.centerFile, target, 'world', w.id))
                                    "
                                >
                                    打开</button
                                ><button
                                    class="btn btn-sm"
                                    :disabled="busy || overview?.running"
                                    @click="perform(() => op({ kind: 'worldExport', world: w.id }))"
                                >
                                    导出 ZIP</button
                                ><button
                                    class="btn btn-sm"
                                    :disabled="busy || overview?.running"
                                    @click="perform(() => op({ kind: 'backup', world: w.id }))"
                                >
                                    备份
                                </button>
                            </div>
                        </div>
                        <p v-if="!filtered.length" class="ic-empty">暂无匹配的存档</p>
                        <button class="btn btn-ghost" @click="chooseTab('backups')">
                            查看备份与恢复 →
                        </button>
                    </template>
                    <template v-else-if="tab === 'screenshots'">
                        <div class="ic-shots">
                            <button
                                v-for="(s, i) in shots"
                                :key="s.id"
                                class="ic-shot"
                                @click="lightbox = i"
                            >
                                <img :src="s.image" loading="lazy" alt="游戏截图" /><span>{{
                                    s.id
                                }}</span>
                            </button>
                        </div>
                        <p v-if="!shots.length" class="ic-empty">
                            游戏中按截图键拍摄后，会显示在这里
                        </p>
                        <div class="ic-tools">
                            <button
                                class="btn btn-sm"
                                :disabled="page === 0 || loading"
                                @click="
                                    page--;
                                    refresh();
                                "
                            >
                                上一页</button
                            ><span
                                >{{ page + 1 }} / {{ Math.max(1, Math.ceil(shotTotal / 24)) }}</span
                            ><button
                                class="btn btn-sm"
                                :disabled="(page + 1) * 24 >= shotTotal || loading"
                                @click="
                                    page++;
                                    refresh();
                                "
                            >
                                下一页
                            </button>
                        </div>
                    </template>
                    <template v-else-if="tab === 'backups'">
                        <div class="ic-summary">
                            <div>
                                <h3>备份与改动保护</h3>
                                <p>
                                    自动保留最近 5 次改动保护；手动备份长期保留。实例备份不包含 Java
                                    与可重新下载的公共运行文件。
                                </p>
                            </div>
                            <button
                                class="btn btn-gold"
                                :disabled="busy || overview?.running"
                                @click="perform(() => op({ kind: 'backup' }))"
                            >
                                备份实例
                            </button>
                        </div>
                        <div v-for="b in backups" :key="b.id" class="ic-row">
                            <div class="ic-grow">
                                <strong>{{ b.title }}</strong
                                ><small
                                    >{{ formatDate(b.createdAt) }} ·
                                    {{ b.automatic ? '自动保护' : '手动备份' }} ·
                                    {{ b.files.length }} 个文件 ·
                                    {{ size(b.files.reduce((n, f) => n + f.size, 0)) }}</small
                                >
                            </div>
                            <button
                                class="btn btn-sm"
                                :disabled="busy || overview?.running"
                                @click="openRestore(b)"
                            >
                                恢复…
                            </button>
                        </div>
                        <p v-if="!backups.length" class="ic-empty">
                            还没有备份，创建第一份备份以保护游戏进度
                        </p>
                    </template>
                    <template v-else-if="tab === 'diagnostics'">
                        <div class="ic-summary">
                            <div>
                                <h3>检查运行环境</h3>
                                <p>
                                    本地分析 Java、运行文件和本实例会话日志。{{
                                        session ? '日志会话：' + formatDate(session) : ''
                                    }}
                                </p>
                            </div>
                            <button class="btn btn-gold" :disabled="busy" @click="diagnose">
                                {{ busy ? '正在处理…' : '开始检查' }}
                            </button>
                        </div>
                        <!-- 二分定位：不知道是哪个 mod 崩时，用「禁一半看还崩不崩」快速收敛 -->
                        <div class="ic-card" data-ui="instance-center:bisect">
                            <h3>二分定位肇事模组</h3>
                            <p v-if="!bisect.status">
                                装了多个模组又不知道是哪个导致崩溃时，用它把范围砍半：每轮禁用一半，
                                启动一次告诉我结果，几轮就能锁定到具体那个。所有改动都是 .jar ↔
                                .jar.disabled 改名，随时可以整体取消并完整复原。
                            </p>
                            <p v-else>
                                第 {{ bisect.round }} 轮 · 剩
                                {{ bisect.candidates.length }} 个候选<template
                                    v-if="bisect.excluded.length"
                                    >，已排除 {{ bisect.excluded.length }} 个</template
                                >
                            </p>
                            <p v-if="bisect.status === 'testing'" class="ic-bisect-next">
                                已禁用 {{ bisect.testing.length }} 个：{{
                                    bisect.testing.slice(0, 4).join('、')
                                }}<template v-if="bisect.testing.length > 4">…</template><br />
                                现在启动游戏：进得去主界面就点「起来了」，仍然崩溃就点「还是崩」。
                            </p>
                            <p v-if="bisect.culprit" class="ic-bisect-found">
                                定位到 {{ bisect.culprit }}
                            </p>
                            <p v-if="bisect.contradiction" class="ic-bisect-warn">
                                {{ bisect.contradiction }}
                            </p>
                            <div class="ic-actions">
                                <template v-if="!bisect.status">
                                    <button
                                        class="btn btn-ghost"
                                        :disabled="busy || bisectBusy || overview?.running"
                                        @click="bisectAct('start')"
                                    >
                                        {{ bisectBusy ? '准备中…' : '开始二分定位' }}
                                    </button>
                                </template>
                                <template v-else-if="bisect.status === 'testing'">
                                    <button
                                        class="btn btn-gold"
                                        :disabled="bisectBusy"
                                        @click="bisectAct('report', { gameStarted: true })"
                                    >
                                        起来了
                                    </button>
                                    <button
                                        class="btn btn-ghost"
                                        :disabled="bisectBusy"
                                        @click="bisectAct('report', { gameStarted: false })"
                                    >
                                        还是崩
                                    </button>
                                    <button
                                        class="btn btn-ghost"
                                        :disabled="bisectBusy"
                                        @click="bisectAct('cancel')"
                                    >
                                        取消并复原
                                    </button>
                                </template>
                                <template v-else>
                                    <button
                                        v-if="bisect.culprit"
                                        class="btn btn-gold"
                                        :disabled="bisectBusy"
                                        @click="bisectAct('finish', { disableCulprit: true })"
                                    >
                                        结束并禁用它
                                    </button>
                                    <button
                                        class="btn btn-ghost"
                                        :disabled="bisectBusy"
                                        @click="bisectAct('finish', { disableCulprit: false })"
                                    >
                                        结束（全部保留）
                                    </button>
                                </template>
                            </div>
                        </div>
                        <div v-for="(f, i) in findings" :key="f.rule + i" class="ic-card">
                            <div class="ic-tools">
                                <strong>{{ f.title }}</strong
                                ><span class="ic-badge">{{
                                    f.confidence === 'certain'
                                        ? '已确认'
                                        : f.confidence === 'possible'
                                          ? '可能原因'
                                          : '证据不足'
                                }}</span>
                            </div>
                            <div v-for="mod in f.mods" :key="mod.fileName" class="ic-row">
                                <input
                                    v-if="
                                        ['duplicate', 'mod-version'].includes(f.rule) &&
                                        !mod.fileName.endsWith('.disabled')
                                    "
                                    v-model="selectedMods"
                                    :value="mod.fileName"
                                    type="checkbox"
                                    :aria-label="'选择停用 ' + mod.fileName"
                                /><img v-if="mod.icon" :src="mod.icon" alt="" />
                                <div class="ic-grow">
                                    <strong>{{ mod.fileName }}</strong
                                    ><small>{{ mod.name }}</small>
                                </div>
                                <button
                                    v-if="f.rule === 'dependency'"
                                    class="btn btn-sm"
                                    :disabled="busy || overview?.running"
                                    @click="checkDependencies(mod.fileName)"
                                >
                                    核对必要前置
                                </button>
                            </div>
                            <pre>{{ f.evidence }}</pre>
                            <p>{{ f.advice }}</p>
                            <div class="ic-actions">
                                <button
                                    v-if="f.action === 'files'"
                                    class="btn btn-sm"
                                    :disabled="busy || overview?.running"
                                    @click="perform(() => op({ kind: 'repair', planId }))"
                                >
                                    校验并修复运行文件
                                </button>
                                <button
                                    v-if="f.action === 'mods'"
                                    class="btn btn-sm"
                                    @click="navigate('mods')"
                                >
                                    打开模组管理
                                </button>
                                <template v-if="f.rule === 'graphics'"
                                    ><button
                                        v-for="driver in [
                                            [
                                                'NVIDIA',
                                                'https://www.nvidia.com/Download/index.aspx'
                                            ],
                                            ['AMD', 'https://www.amd.com/en/support'],
                                            [
                                                'Intel',
                                                'https://www.intel.com/content/www/us/en/download-center/home.html'
                                            ]
                                        ]"
                                        :key="driver[0]"
                                        class="btn btn-sm"
                                        @click="invoke('app:openExternal', driver[1])"
                                    >
                                        {{ driver[0] }} 官方驱动
                                    </button></template
                                >
                            </div>
                        </div>
                        <button
                            v-if="selectedMods.length"
                            class="btn btn-gold"
                            :disabled="busy || overview?.running"
                            @click="form = 'disable'"
                        >
                            停用选中的 {{ selectedMods.length }} 个模组…
                        </button>
                        <div v-if="findings.some((f) => f.action === 'java')" class="ic-card">
                            <h3>选择适配 Java</h3>
                            <SelectMenu
                                v-model="javaPath"
                                :options="
                                    java.map((j) => ({
                                        value: j.path,
                                        label: 'Java ' + j.major + ' · ' + j.path
                                    }))
                                "
                                placeholder="尚未发现兼容运行时"
                            />
                            <div class="ic-actions">
                                <button
                                    class="btn btn-gold"
                                    :disabled="busy || !javaPath || overview?.running"
                                    @click="
                                        perform(() =>
                                            setVersionJava(
                                                target.id,
                                                javaPath,
                                                false,
                                                target.folder
                                            )
                                        )
                                    "
                                >
                                    为此实例应用</button
                                ><button
                                    class="btn btn-ghost"
                                    @click="
                                        close();
                                        openSettings('java');
                                    "
                                >
                                    管理 Java
                                </button>
                            </div>
                        </div>
                        <button
                            class="btn btn-ghost"
                            :disabled="busy"
                            @click="perform(() => exportLaunchLogs(target.id, target.folder))"
                        >
                            导出脱敏诊断日志
                        </button>
                    </template>
                    <p v-if="busy" class="ic-hint" role="status">
                        任务正在执行。进度及取消入口位于下载中心，关闭本页面不会取消任务。
                    </p>
                </div>
                <div v-if="form" class="ic-dialog-mask">
                    <section class="ic-dialog" role="dialog" aria-label="实例操作确认">
                        <header>
                            <h3>
                                {{
                                    form === 'clone'
                                        ? '复制实例'
                                        : form === 'restore'
                                          ? '恢复备份'
                                          : form === 'disable'
                                            ? '确认停用模组'
                                            : form === 'dependencies'
                                              ? '安装必要前置'
                                              : '导入存档'
                                }}
                            </h3>
                            <button class="btn btn-sm" aria-label="关闭操作确认" @click="form = ''">
                                <UiGlyph name="close" />
                            </button>
                        </header>
                        <template v-if="form === 'disable'"
                            ><p>
                                仅停用你勾选的文件。先创建自动保护记录，可在“备份”中恢复。请结合日志核对需要保留的版本。
                            </p>
                            <pre>{{ selectedMods.join('\n') }}</pre>
                        </template>
                        <template v-else-if="form === 'dependencies'"
                            ><p>{{ dependencies?.changelog }}</p>
                            <div v-for="f in dependencies?.files" :key="f.fileName" class="ic-row">
                                <strong>{{ f.fileName }}</strong
                                ><small>{{ f.version }}</small>
                            </div></template
                        >
                        <label v-else-if="!overwrite || restore?.metadata?.type === 'world'"
                            >名称<input v-model="name" class="input" maxlength="120"
                        /></label>
                        <template
                            v-if="
                                form === 'clone' ||
                                (form === 'restore' &&
                                    restore?.metadata?.type !== 'world' &&
                                    !overwrite)
                            "
                            ><label
                                >目标游戏文件夹<SelectMenu
                                    v-model="destination"
                                    :options="folders" /></label
                        ></template>
                        <template v-if="form === 'clone'"
                            ><label class="ic-check"
                                ><input v-model="includeSaves" type="checkbox" />包含存档</label
                            ><label class="ic-check"
                                ><input v-model="includeShots" type="checkbox" />包含截图</label
                            >
                            <p v-if="overview?.shared" class="ic-hint">
                                当前为共享目录，以下范围会复制到新隔离实例。
                            </p>
                            <details>
                                <summary>查看复制范围</summary>
                                <p>{{ overview?.roots.join('、') }}</p>
                            </details></template
                        >
                        <template v-if="form === 'restore'"
                            ><p>
                                校验备份后恢复为新的{{
                                    restore?.metadata?.type === 'world' ? '存档' : '隔离实例'
                                }}。恢复自动保护记录时，会复制当前实例并还原受影响文件。
                            </p>
                            <label
                                v-if="restore?.metadata?.type === 'world' || !overview?.shared"
                                class="ic-check"
                                ><input v-model="overwrite" type="checkbox" />覆盖{{
                                    restore?.metadata?.type === 'world' ? '同名存档' : '当前实例'
                                }}（先备份现有内容）</label
                            ></template
                        >
                        <template v-if="form === 'import'"
                            ><SelectMenu
                                v-model="candidate"
                                :options="
                                    (worldInfo?.candidates || []).map((c) => ({
                                        value: c.id,
                                        label:
                                            c.worldName + ' · ' + (c.minecraftVersion || '版本未知')
                                    }))
                                "
                            /><label class="ic-check"
                                ><input
                                    v-model="mismatch"
                                    type="checkbox"
                                />确认允许版本不匹配（建议先备份）</label
                            ></template
                        >
                        <p v-if="error" class="ic-error">{{ error }}</p>
                        <footer>
                            <button class="btn btn-ghost" @click="form = ''">取消</button
                            ><button
                                class="btn btn-gold"
                                :disabled="
                                    busy ||
                                    (form === 'disable'
                                        ? !selectedMods.length
                                        : form === 'dependencies'
                                          ? !dependencies
                                          : !name.trim())
                                "
                                @click="
                                    perform(() =>
                                        form === 'dependencies'
                                            ? invoke(
                                                  'center:dependencyApply',
                                                  dependencies?.id,
                                                  true
                                              )
                                            : form === 'disable'
                                              ? disableSelected()
                                              : form === 'clone'
                                                ? op({
                                                      kind: 'clone',
                                                      name,
                                                      destinationFolder: destination,
                                                      saves: includeSaves,
                                                      screenshots: includeShots
                                                  })
                                                : form === 'restore'
                                                  ? op({
                                                        kind: 'restore',
                                                        name,
                                                        destinationFolder: destination,
                                                        backupId: restore?.id,
                                                        overwrite
                                                    })
                                                  : importWorld(worldInput, {
                                                        candidateId: candidate,
                                                        worldName: name,
                                                        targetFolder: target.folder,
                                                        targetVersionId: target.id,
                                                        allowVersionMismatch: mismatch
                                                    })
                                    )
                                "
                            >
                                {{ busy ? '正在处理…' : '确认执行' }}
                            </button>
                        </footer>
                    </section>
                </div>
            </section>
        </div>
        <div v-if="image" class="ic-lightbox" role="dialog" aria-label="查看截图">
            <header>
                <strong>{{ image.id }}</strong
                ><button class="btn" @click="lightbox = -1">关闭</button>
            </header>
            <img :src="fullImage || image.image" alt="游戏截图大图" />
            <footer>
                <button class="btn" :disabled="lightbox === 0" @click="lightbox--">上一张</button
                ><button
                    class="btn"
                    @click="
                        perform(() => invoke(IPC.centerFile, target, 'screenshot', image.id, true))
                    "
                >
                    另存为</button
                ><button
                    class="btn"
                    @click="perform(() => invoke(IPC.centerFile, target, 'screenshot', image.id))"
                >
                    打开所在位置</button
                ><button class="btn" :disabled="lightbox === shots.length - 1" @click="lightbox++">
                    下一张
                </button>
            </footer>
        </div>
    </Teleport>
</template>
<style scoped>
.ic-mask {
    position: fixed;
    inset: 0;
    z-index: 9500;
    background: #0009;
    backdrop-filter: blur(8px);
    padding: 28px;
    display: grid;
    place-items: center;
}
.ic {
    position: relative;
    display: flex;
    flex-direction: column;
    width: min(1120px, 100%);
    height: min(860px, 100%);
    background: var(--card-solid, #192225);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 24px;
    box-shadow: var(--shadow-lg);
    overflow: hidden;
}
.ic-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    padding: 26px 30px 18px;
    gap: 24px;
}
.ic-header h2 {
    margin: 6px 0;
    font-size: 26px;
    overflow-wrap: anywhere;
}
.ic-header p,
.ic-header small {
    color: var(--text-dim);
    margin: 0;
}
.ic-tabs {
    display: flex;
    gap: 6px;
    padding: 0 30px 16px;
    border-bottom: 1px solid var(--border);
    overflow: auto;
    flex-shrink: 0;
}
.ic-tabs button {
    border: 0;
    background: transparent;
    color: var(--text-dim);
    padding: 10px 24px;
    white-space: nowrap;
    border-radius: 12px;
    font: inherit;
    cursor: pointer;
}
.ic-tabs button.active {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 700;
}
.ic-content {
    padding: 24px 30px;
    overflow: auto;
    min-height: 0;
    overscroll-behavior: contain;
}
.ic-summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    margin-bottom: 24px;
}
.ic-summary h3 {
    margin: 0 0 10px;
}
.ic p {
    line-height: 1.65;
    color: var(--text-dim);
}
.ic-path {
    overflow-wrap: anywhere;
    font-size: 12px;
}
.ic-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
    margin-bottom: 22px;
}
.ic-card {
    display: block;
    min-width: 0;
    text-align: left;
    padding: 20px;
    border: 1px solid var(--border);
    border-radius: 16px;
    color: var(--text);
    background: var(--card);
    margin-bottom: 14px;
    font: inherit;
}
/* 导出整合包：两个并列按钮等宽等距（相邻元素等长，避免一长一短） */
.ic-export-actions {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
}
.ic-export-actions > .btn {
    flex: 1 1 0;
    min-width: 150px;
    justify-content: center;
}
/* 二分定位的下一步提示：与正文区分开，读起来像「该你操作了」 */
.ic-bisect-next {
    color: var(--text) !important;
    border-left: 2px solid var(--accent);
    padding-left: var(--space-3);
}
.ic-bisect-found {
    color: var(--accent);
    font-weight: 600;
}
.ic-bisect-warn {
    color: var(--danger);
}
.ic-grid .ic-card {
    margin: 0;    cursor: pointer;
    transition:
        background 0.18s,
        transform 0.18s;
}
.ic-grid .ic-card:hover {
    background: var(--accent-soft);
    transform: translateY(-2px);
}
.ic-card span {
    display: block;
    color: var(--text-dim);
    font-size: 13px;
    margin-top: 8px;
}
.ic-row {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 18px 0;
    border-bottom: 1px solid var(--border);
}
.ic-row img,
.ic-world-icon {
    width: 58px;
    height: 58px;
    object-fit: cover;
    border-radius: 10px;
    flex-shrink: 0;
}
.ic-world-icon {
    display: grid;
    place-items: center;
    background: var(--accent-soft);
    font-size: 28px;
}
.ic-grow {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
}
.ic-grow small {
    display: block;
    color: var(--text-dim);
    margin-top: 8px;
}
.ic-actions,
.ic-tools {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
}
.ic-tools {
    margin-bottom: 16px;
}
.ic-tools .input {
    flex: 1;
    min-width: 180px;
}
.ic-shots {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 16px;
    margin-bottom: 20px;
}
.ic-shot {
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 0;
    overflow: hidden;
    background: var(--card);
    color: var(--text);
    cursor: pointer;
}
.ic-shot img {
    width: 100%;
    aspect-ratio: 16/9;
    object-fit: cover;
}
.ic-shot span {
    display: block;
    padding: 10px;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
}
.ic-empty {
    text-align: center;
    padding: 56px 12px;
}
.ic-error {
    color: var(--danger) !important;
    overflow-wrap: anywhere;
}
.ic-hint {
    background: var(--accent-soft);
    padding: 12px 16px;
    border-radius: 12px;
}
.ic pre {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 180px;
    overflow: auto;
    background: var(--bg);
    padding: 14px;
    border-radius: 10px;
    font-size: 12px;
}
.ic-badge {
    font-size: 12px !important;
    padding: 4px 8px;
    border-radius: 8px;
    background: var(--accent-soft);
    margin: 0 !important;
}
.ic-dialog-mask {
    position: absolute;
    inset: 0;
    background: #0008;
    display: grid;
    place-items: center;
    padding: 24px;
    z-index: 2;
}
.ic-dialog {
    background: var(--card-solid, #192225);
    border: 1px solid var(--border);
    border-radius: 18px;
    padding: 24px;
    width: min(560px, 100%);
    max-height: 100%;
    overflow: auto;
}
.ic-dialog header,
.ic-dialog footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
}
.ic-dialog footer {
    justify-content: flex-end;
    margin-top: 20px;
}
.ic-dialog label {
    display: block;
    margin: 18px 0;
}
.ic-dialog label .input {
    display: block;
    width: 100%;
    margin-top: 8px;
    box-sizing: border-box;
}
.ic-dialog .ic-check {
    display: flex;
    align-items: center;
    gap: 10px;
}
.ic-lightbox {
    position: fixed;
    inset: 0;
    z-index: 10000;
    background: #101418fa;
    color: white;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 24px;
}
.ic-lightbox header,
.ic-lightbox footer {
    display: flex;
    gap: 16px;
    align-items: center;
}
.ic-lightbox img {
    min-height: 0;
    flex: 1;
    max-width: 100%;
    object-fit: contain;
}
.ic-lightbox .btn {
    color: white;
    background: #293139;
}
@media (max-width: 800px) {
    .ic-mask {
        padding: 10px;
    }
    .ic-header,
    .ic-content {
        padding: 18px;
    }
    .ic-tabs {
        padding: 0 14px 14px;
    }
    .ic-tabs button {
        padding: 10px 15px;
    }
    .ic-row,
    .ic-summary {
        flex-wrap: wrap;
    }
    .ic-shots {
        grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .ic-header h2 {
        font-size: 21px;
    }
    .ic-actions {
        width: 100%;
    }
}
@media (prefers-reduced-motion: reduce) {
    .ic-grid .ic-card {
        transition: none;
    }
}
</style>

<style scoped>
.ic-note {
    margin: 6px 0 0;
    font-size: 12.5px;
    line-height: 1.6;
    color: var(--text-dim);
    cursor: pointer;
    max-width: 560px;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
}
.ic-note:hover {
    color: var(--text);
}
.ic-note em {
    color: var(--text-dim);
    opacity: 0.7;
}
.ic-note-edit {
    display: grid;
    gap: 8px;
    margin-top: 6px;
    max-width: 560px;
}
.ic-note-edit textarea {
    resize: vertical;
    min-height: 56px;
    padding: 8px 10px;
    border-radius: var(--radius-md, 12px);
    border: 1px solid var(--border);
    background: var(--bg-2);
    color: var(--text);
    font: inherit;
    font-size: 12.5px;
    outline: none;
}
.ic-note-edit textarea:focus {
    border-color: var(--accent);
}
.ic-note-edit {
    justify-items: end;
}
.ic-note-edit textarea {
    justify-self: stretch;
}
</style>
