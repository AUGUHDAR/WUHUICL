<script setup lang="ts">
/**
 * 资源详情同页展示（用户拍板：砍掉 inlineWeb 子进程窗口，与美西同页展示）。
 * 布局模型抄 PCL PageDownloadCompDetail（资源信息卡 + 版本分组筛选 + 版本分组折叠列表 + 行内下载）：
 * - variant="page"（默认）：启动器内整页视图，由 CommunityView 详情子状态渲染；
 * - variant="modal"：居中小窗（安装确认弹窗内查看前置项目时沿用，无版本列表）。
 * 版本分组口径：快照/非正式版本号归「快照版」；分组超过 8 组时按主次版本折叠（PCL GroupedDrop 同意图）。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import UiGlyph from './UiGlyph.vue';
import TabPills from './TabPills.vue';
import EmptyState from './EmptyState.vue';
import { communityFiles, communityProject, copyText, errText } from '../api';
import { toast } from '../store';
import { favorites, favoriteBusy, toggleProject } from '../modFavorites';
import { usesCommunityLoader } from '@shared/communityPolicy';
import type { CommunityFile, CommunityModProject, CommunityProjectReference } from '@shared/types';

const props = withDefaults(
    defineProps<{
        reference: CommunityProjectReference;
        allowDownload?: boolean;
        /** 渲染形态：page = 启动器内整页（默认）；modal = 居中小窗（弹窗内查看前置项目） */
        variant?: 'page' | 'modal';
        /** 默认展开与当前筛选一致的 MC 版本分组（CommunityView 传入 query.mcVersion） */
        matchVersion?: string;
    }>(),
    { allowDownload: true, variant: 'page', matchVersion: '' }
);
const emit = defineEmits<{
    (event: 'close'): void;
    /** modal 形态沿用：整包跳下载流程 */
    (event: 'download', project: CommunityProjectReference): void;
    /** page 形态：行内「下载」——宿主页用该文件预选打开下载弹窗（目标实例选择仍走弹窗） */
    (event: 'download-file', fileId: string): void;
    /** page 形态：MC 百科入口交给宿主页（复用其词典/百科桥检索逻辑） */
    (event: 'wiki', project: CommunityModProject): void;
}>();

const project = ref<CommunityModProject>(),
    loading = ref(false),
    error = ref(''),
    retry = ref(0);
watch(
    [() => props.reference.source, () => props.reference.projectId, retry],
    async (_next, _old, cleanup) => {
        let stale = false;
        cleanup(() => {
            stale = true;
        });
        loading.value = true;
        error.value = '';
        project.value = undefined;
        try {
            const result = await communityProject(
                props.reference.source,
                props.reference.projectId
            );
            if (!stale) project.value = result;
        } catch (reason) {
            if (!stale) error.value = errText(reason);
        } finally {
            if (!stale) loading.value = false;
        }
    },
    { immediate: true }
);
const date = (value: string) => {
    const parsed = new Date(value);
    return Number.isNaN(parsed.getTime()) ? '' : parsed.toLocaleDateString('zh-CN');
};
const fmtSize = (bytes: number): string => {
    if (!bytes || bytes <= 0) return '—';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
};
const releaseTagClass = (t: CommunityFile['releaseType']) =>
    t === 'beta' ? 'tag-cyan' : t === 'alpha' ? 'tag-danger' : 'tag-gold';
const releaseText: Record<CommunityFile['releaseType'], string> = {
    release: '正式版',
    beta: 'Beta',
    alpha: 'Alpha'
};

// ---------------- 收藏（page 形态信息卡操作） ----------------
const itemKey = (r: CommunityProjectReference) => `${r.source}:${r.projectId}`;
const isFavorited = computed(() => !!project.value && favorites.value.some((f) => f.key === itemKey(project.value!)));
function toggleFavorite() {
    const p = project.value;
    if (!p || favoriteBusy.value.has(itemKey(p))) return;
    toggleProject(p.source, p.projectId, p.title, p.iconUrl);
}

// ---------------- 源页面 / 复制名称 / 百科 ----------------
/** 详情接口自带 webpage；Modrinth 缺失时用 slug/projectId 兜底构造（CurseForge 需要分类段，不猜） */
const sourceLink = computed(() => {
    if (project.value?.webpage) return project.value.webpage;
    return props.reference.source === 'modrinth'
        ? `https://modrinth.com/project/${props.reference.slug || props.reference.projectId}`
        : '';
});
function openSource() {
    if (sourceLink.value) window.open(sourceLink.value, '_blank');
}
async function copyName() {
    const name = project.value?.title || props.reference.title;
    try {
        toast((await copyText(name)) ? '名称已复制' : '复制失败', 'info');
    } catch {
        toast('复制失败', 'error');
    }
}

// ---------------- 版本文件列表（PCL：全量文件 → 版本分组折叠卡片） ----------------
const files = ref<CommunityFile[]>([]);
const filesLoading = ref(false),
    filesError = ref('');
const fileRetry = ref(0);
watch(
    [() => props.reference.source, () => props.reference.projectId, fileRetry],
    async (_next, _old, cleanup) => {
        if (props.variant !== 'page') return;
        let stale = false;
        cleanup(() => {
            stale = true;
        });
        filesLoading.value = true;
        filesError.value = '';
        files.value = [];
        try {
            const list = await communityFiles(props.reference.source, props.reference.projectId);
            if (!stale) files.value = list;
        } catch (reason) {
            if (!stale) filesError.value = errText(reason);
        } finally {
            if (!stale) filesLoading.value = false;
        }
    },
    { immediate: true }
);

/** PCL GetGroupedVersionName 口径：完整 x.y / x.y.z 为正式分组，其余（快照/预发布）归「快照版」 */
function versionGroupKey(v: string): string {
    const s = v.trim();
    return /^\d+\.\d+(\.\d+)?$/.test(s) ? s : '快照版';
}
/** 版本号降序比较（缺段补 0） */
function cmpVersion(a: string, b: string): number {
    const pa = a.split('.').map(Number);
    const pb = b.split('.').map(Number);
    for (let i = 0; i < Math.max(pa.length, pb.length); i += 1) {
        const x = pa[i] ?? 0;
        const y = pb[i] ?? 0;
        if (x !== y) return y - x;
    }
    return 0;
}
function groupRank(key: string): number {
    const version = key.replace(/^\S+\s/, '');
    if (version === '快照版') return 2;
    return versionGroupKey(version) === '快照版' ? 2 : 0;
}

/** 版本筛选（PCL CardFilter 模型）：全部 + 分组名；分组超过 8 组时按主次版本折叠 */
const groupFilter = ref('');
const foldNeeded = computed(() => {
    const names = new Set<string>();
    for (const f of files.value) for (const v of f.gameVersions) names.add(versionGroupKey(v));
    return names.size > 8;
});
function foldName(name: string): string {
    if (!foldNeeded.value) return name;
    const m = name.match(/^(\d+\.\d+)\.\d+$/);
    return m ? m[1] : name;
}
const filterItems = computed(() => {
    const names = new Set<string>();
    for (const f of files.value)
        for (const v of f.gameVersions) {
            const key = versionGroupKey(v);
            if (key !== '快照版') names.add(foldName(key));
        }
    const list = [...names].sort(cmpVersion);
    if (files.value.some((f) => f.gameVersions.some((v) => versionGroupKey(v) === '快照版')))
        list.push('快照版');
    return [{ value: '', label: '全部' }, ...list.map((n) => ({ value: n, label: n }))];
});

interface FileGroup {
    key: string;
    version: string;
    files: CommunityFile[];
}
const groups = computed<FileGroup[]>(() => {
    const loaderSet = new Set<string>();
    for (const f of files.value) for (const l of f.loaders) loaderSet.add(l);
    const multiLoader = loaderSet.size > 1; // PCL：工程存在多个加载器时按加载器分卡
    const map = new Map<string, CommunityFile[]>();
    for (const f of files.value) {
        const loaderKeys = multiLoader && f.loaders.length ? f.loaders : [''];
        for (const gv of f.gameVersions) {
            const version = versionGroupKey(gv);
            if (groupFilter.value && foldName(version) !== groupFilter.value) continue;
            for (const lk of loaderKeys) {
                const key = lk ? `${lk} ${version}` : version;
                const arr = map.get(key) ?? [];
                if (!arr.some((x) => x.fileId === f.fileId)) arr.push(f);
                map.set(key, arr);
            }
        }
    }
    return [...map.entries()]
        .map(([key, fs]) => ({ key, version: key.replace(/^\S+\s/, ''), files: fs }))
        .sort((a, b) => {
            const ra = groupRank(a.key);
            const rb = groupRank(b.key);
            if (ra !== rb) return rb - ra; // 正式版在前，快照版垫底
            if (ra === 2) return a.key.localeCompare(b.key);
            return cmpVersion(a.version, b.version);
        });
});

/** 展开状态（PCL：所选版本卡默认展开，其余收起；仅一张卡时展开它） */
const expanded = ref<Set<string>>(new Set());
function defaultExpanded(): Set<string> {
    const set = new Set<string>();
    const all = groups.value.map((g) => g.key);
    if (all.length <= 1) {
        all.forEach((k) => set.add(k));
        return set;
    }
    if (props.matchVersion) {
        const hit = all.find((k) => k === props.matchVersion || k.endsWith(` ${props.matchVersion}`));
        if (hit) {
            set.add(hit);
            return set;
        }
        // 版本筛选命中时展开该筛选下的第一张卡
        if (groupFilter.value) {
            set.add(all[0]);
            return set;
        }
    }
    set.add(all[0]);
    return set;
}
watch(
    () => [groups.value.map((g) => g.key).join('|'), groupFilter.value],
    () => {
        expanded.value = defaultExpanded();
    },
    { immediate: false }
);
/** <details> 原生开合与 expanded 集合双向同步（chevron 跟随状态） */
function syncGroup(key: string, open: boolean) {
    const next = new Set(expanded.value);
    open ? next.add(key) : next.delete(key);
    expanded.value = next;
}

// ---------------- page 形态：ESC 返回列表（UX「回车与 ESC 最好有响应」） ----------------
function onEsc(event: KeyboardEvent) {
    if (event.key === 'Escape') {
        event.preventDefault();
        emit('close');
    }
}
onMounted(() => {
    if (props.variant === 'page') window.addEventListener('keydown', onEsc);
});
onBeforeUnmount(() => window.removeEventListener('keydown', onEsc));
</script>

<template>
    <!-- page 形态：启动器内整页视图 -->
    <section
        v-if="variant === 'page'"
        class="community-detail-page"
        role="region"
        aria-label="资源详情"
        data-ui="community:project-details"
        :aria-busy="loading || filesLoading"
    >
        <!-- 返回条 -->
        <div class="detail-topbar">
            <button class="btn btn-ghost btn-sm" @click="emit('close')">
                <UiGlyph name="arrow-left" :size="14" /> 返回列表
            </button>
            <span class="muted detail-platform"
                >{{ reference.source === 'modrinth' ? 'Modrinth' : 'CurseForge' }} · MOD 详情</span
            >
        </div>

        <!-- 资源信息卡（PCL CardIntro/MyResourceItem 模型：logo + 标题 + 事实 + 操作按钮） -->
        <div class="card detail-intro">
            <div v-if="loading" class="detail-loading muted" role="status">
                <span class="spin" />正在读取来源项目资料…
            </div>
            <div v-else-if="error" class="detail-error" role="alert">
                <p>{{ error }}</p>
                <button class="btn btn-ghost btn-sm" @click="retry++">重试</button>
            </div>
            <template v-else-if="project">
                <div class="detail-intro-top">
                    <span class="detail-logo" aria-hidden="true">
                        <img
                            v-if="project.iconUrl"
                            :src="project.iconUrl"
                            alt=""
                            loading="lazy"
                            referrerpolicy="no-referrer"
                            @error="($event.target as HTMLImageElement).style.display = 'none'"
                        />
                        <span v-else class="detail-logo-fallback">{{
                            (project.title || '?').charAt(0).toUpperCase()
                        }}</span>
                    </span>
                    <div class="detail-title-wrap">
                        <h2 class="detail-title">{{ project.title || reference.title }}</h2>
                        <p v-if="project.description" class="detail-summary" data-ui="community:project-summary">
                            {{ project.description }}
                        </p>
                        <p v-else class="muted detail-summary">来源平台未提供摘要。</p>
                    </div>
                </div>
                <dl class="detail-facts">
                    <div v-if="project.author">
                        <dt>作者</dt>
                        <dd>{{ project.author }}</dd>
                    </div>
                    <div v-if="project.license">
                        <dt>许可证</dt>
                        <dd>{{ project.license }}</dd>
                    </div>
                    <div v-if="project.downloads !== undefined" data-ui="community:project-downloads">
                        <dt>下载量</dt>
                        <dd>{{ project.downloads.toLocaleString('zh-CN') }}</dd>
                    </div>
                    <div v-if="project.followers !== undefined">
                        <dt>关注人数</dt>
                        <dd>{{ project.followers.toLocaleString('zh-CN') }}</dd>
                    </div>
                    <div v-if="project.updatedAt && date(project.updatedAt)">
                        <dt>更新日期</dt>
                        <dd>{{ date(project.updatedAt) }}</dd>
                    </div>
                </dl>
                <div v-if="project.categories.length" class="detail-categories" aria-label="项目类别">
                    <span v-for="category in project.categories" :key="category" class="tag">{{
                        category
                    }}</span>
                </div>
                <div class="detail-actions">
                    <button
                        v-if="sourceLink"
                        class="btn btn-ghost btn-sm"
                        @click="openSource"
                    >
                        <UiGlyph name="expand" :size="14" />
                        转到 {{ reference.source === 'modrinth' ? 'Modrinth' : 'CurseForge' }}
                    </button>
                    <button class="btn btn-ghost btn-sm" @click="emit('wiki', project)">
                        转到 MC 百科
                    </button>
                    <button class="btn btn-ghost btn-sm" @click="copyName">复制名称</button>
                    <button
                        class="btn btn-ghost btn-sm detail-favorite"
                        :class="{ active: isFavorited }"
                        :aria-pressed="isFavorited"
                        :disabled="favoriteBusy.has(itemKey(project))"
                        @click="toggleFavorite"
                    >
                        <UiGlyph name="star" :size="14" :filled="isFavorited" />
                        {{ isFavorited ? '已收藏' : '收藏模组' }}
                    </button>
                </div>
            </template>
        </div>

        <!-- 版本分组列表（PCL：筛选药丸 + 每组一张折叠卡） -->
        <div class="card detail-versions">
            <div class="detail-versions-head">
                <strong>版本列表</strong>
                <span v-if="files.length" class="muted">共 {{ files.length }} 个文件</span>
            </div>
            <div v-if="filesLoading" class="detail-loading muted" role="status">
                <span class="spin" />正在获取版本列表…
            </div>
            <div v-else-if="filesError" class="detail-error" role="alert">
                <p>{{ filesError }}</p>
                <button class="btn btn-ghost btn-sm" @click="fileRetry++">重试</button>
            </div>
            <template v-else-if="files.length">
                <TabPills
                    v-if="filterItems.length > 2"
                    v-model="groupFilter"
                    :items="filterItems"
                    aria-label="版本筛选"
                    class="detail-filter"
                />
                <template v-if="groups.length">
                    <details
                        v-for="g in groups"
                        :key="g.key"
                        class="detail-group"
                        :open="expanded.has(g.key)"
                        @toggle="syncGroup(g.key, ($event.target as HTMLDetailsElement).open)"
                    >
                        <summary class="detail-group-head">
                            <span class="detail-group-title">
                                <strong>{{ g.key }}</strong>
                                <small class="muted">{{ g.files.length }} 个文件</small>
                            </span>
                            <UiGlyph
                                name="chevron-down"
                                :size="14"
                                class="detail-group-chevron"
                                :class="{ open: expanded.has(g.key) }"
                            />
                        </summary>
                        <div class="detail-group-body">
                            <button
                                v-for="f in g.files"
                                :key="f.fileId"
                                class="file-row"
                                :title="f.fileName"
                                @click="emit('download-file', f.fileId)"
                            >
                                <span class="file-main">
                                    <span class="file-name">{{ f.fileName }}</span>
                                    <span class="file-sub"
                                        >版本 {{ f.version }} · MC
                                        {{ f.gameVersions.join(' / ') }}<template
                                            v-if="usesCommunityLoader('mod') && f.loaders.length"
                                        >
                                            · {{ f.loaders.join(' / ') }}</template
                                        ></span
                                    >
                                </span>
                                <span class="file-side">
                                    <span class="tag" :class="releaseTagClass(f.releaseType)">{{
                                        releaseText[f.releaseType]
                                    }}</span>
                                    <span class="muted file-meta"
                                        >{{ date(f.date) }} · {{ fmtSize(f.size) }}</span
                                    >
                                    <span class="btn btn-gold btn-sm file-dl"
                                        ><UiGlyph name="download" :size="13" />下载</span
                                    >
                                </span>
                            </button>
                        </div>
                    </details>
                    <!-- PCL 同款提示：刚更新的版本可能暂时识别不到 -->
                    <p v-if="groups.some((g) => g.version === '快照版')" class="muted detail-note">
                        「快照版」收录暂未归入正式版本号的文件；刚更新的版本几天后即可正常识别。
                    </p>
                </template>
                <EmptyState
                    v-else
                    data-ui="community:detail-files-empty"
                    icon="warning"
                    title="没有匹配该筛选的版本文件"
                    description="换个版本筛选，或在下载弹窗中手动调整 Minecraft / Loader 条件。"
                />
            </template>
            <EmptyState
                v-else
                data-ui="community:detail-files-empty"
                icon="download"
                title="来源平台没有可下载的版本文件"
                description="该项目可能已下架或尚未发布兼容文件。"
            />
        </div>
    </section>

    <!-- modal 形态：居中小窗（弹窗内查看前置项目沿用） -->
    <Teleport v-else to="body"
        ><div
            class="modal-mask"
            style="z-index: var(--z-modal)"
            @pointerdown.self="emit('close')"
            @keydown.esc.prevent="emit('close')"
        >
            <section
                class="modal community-project-modal"
                role="dialog"
                aria-modal="true"
                aria-labelledby="community-project-title"
                data-ui="community:project-details"
                :aria-busy="loading"
            >
                <header class="project-heading">
                    <div>
                        <span class="project-platform muted"
                            >{{ reference.source === 'modrinth' ? 'Modrinth' : 'CurseForge' }} · MOD
                            详情</span
                        >
                        <h3 id="community-project-title" class="modal-title">
                            {{ project?.title || reference.title }}
                        </h3>
                    </div>
                    <button
                        class="icon-btn"
                        data-modal-dismiss
                        aria-label="关闭模组详情"
                        @click="emit('close')"
                    >
                        <UiGlyph name="close" :size="14" />
                    </button>
                </header>
                <div v-if="loading" class="project-loading muted" role="status">
                    <span class="spin" />正在读取来源项目资料…
                </div>
                <div v-else-if="error" class="project-error" role="alert">
                    <p>{{ error }}</p>
                    <button class="btn btn-ghost btn-sm" @click="retry++">重试</button>
                </div>
                <template v-else-if="project">
                    <p
                        v-if="project.description"
                        class="project-summary"
                        data-ui="community:project-summary"
                    >
                        {{ project.description }}
                    </p>
                    <p v-else class="muted project-summary">来源平台未提供摘要。</p>
                    <dl class="project-facts">
                        <div>
                            <dt>项目 ID</dt>
                            <dd>{{ project.projectId }}</dd>
                        </div>
                        <div v-if="project.author">
                            <dt>作者</dt>
                            <dd>{{ project.author }}</dd>
                        </div>
                        <div v-if="project.license">
                            <dt>许可证</dt>
                            <dd>{{ project.license }}</dd>
                        </div>
                        <div
                            v-if="project.downloads !== undefined"
                            data-ui="community:project-downloads"
                        >
                            <dt>下载量</dt>
                            <dd>{{ project.downloads.toLocaleString('zh-CN') }}</dd>
                        </div>
                        <div v-if="project.followers !== undefined">
                            <dt>关注人数</dt>
                            <dd>{{ project.followers.toLocaleString('zh-CN') }}</dd>
                        </div>
                        <div v-if="project.updatedAt && date(project.updatedAt)">
                            <dt>更新日期</dt>
                            <dd>{{ date(project.updatedAt) }}</dd>
                        </div>
                    </dl>
                    <div
                        v-if="project.categories.length"
                        class="project-categories"
                        aria-label="项目类别"
                    >
                        <span v-for="category in project.categories" :key="category" class="tag">{{
                            category
                        }}</span>
                    </div>
                </template>
                <footer class="modal-actions">
                    <button
                        v-if="project?.webpage"
                        class="btn btn-ghost project-source"
                        @click="openSource"
                    >
                        打开来源页面</button
                    ><button class="btn btn-ghost" data-modal-dismiss @click="emit('close')">
                        关闭</button
                    ><button
                        v-if="project && allowDownload"
                        class="btn btn-gold project-download"
                        @click="project && emit('download', project)"
                    >
                        选择版本并安装
                    </button>
                </footer>
            </section>
        </div></Teleport
    >
</template>

<style scoped>
/* ---------- page 形态（同页整页视图） ---------- */
.community-detail-page {
    display: flex;
    flex-direction: column;
    gap: var(--card-gap);
    min-width: 0;
}
.detail-topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
}
.detail-platform {
    font-size: var(--text-xs);
}
.detail-intro {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
}
.detail-intro-top {
    display: flex;
    align-items: flex-start;
    gap: var(--space-4);
    min-width: 0;
}
.detail-logo {
    display: grid;
    place-items: center;
    width: 56px;
    height: 56px;
    flex: none;
    border: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
    border-radius: var(--radius-md);
    overflow: hidden;
}
.detail-logo img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
}
.detail-logo-fallback {
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--text-dim);
}
.detail-title-wrap {
    min-width: 0;
    flex: 1;
}
.detail-title {
    margin: 0;
    font-size: var(--text-2xl);
    line-height: 1.4;
    overflow-wrap: anywhere;
}
.detail-summary {
    margin: var(--space-2) 0 0;
    font-size: var(--text-sm);
    line-height: 1.7;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
}
.detail-facts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: var(--space-3) var(--space-5);
    margin: 0;
    padding: var(--space-3) 0;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
}
.detail-facts dt {
    font-size: var(--text-xs);
    color: var(--text-dim);
    margin-bottom: 4px;
}
.detail-facts dd {
    margin: 0;
    font-size: var(--text-sm);
    line-height: 1.6;
    overflow-wrap: anywhere;
}
.detail-categories {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
}
.detail-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
}
.detail-favorite.active {
    color: var(--accent-2);
    border-color: var(--accent);
}
.detail-versions {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
}
.detail-versions-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-md);
}
.detail-versions-head strong {
    font-weight: 600;
}
.detail-filter {
    align-self: flex-start;
}
.detail-group {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: transparent;
    overflow: hidden;
}
.detail-group[open] {
    border-color: var(--border-strong);
}
.detail-group-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    cursor: pointer;
    list-style: none;
    transition: background 160ms ease;
}
.detail-group-head::-webkit-details-marker {
    display: none;
}
.detail-group-head:hover {
    background: var(--hover);
}
.detail-group-title {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    min-width: 0;
}
.detail-group-title strong {
    font-size: var(--text-md);
    overflow-wrap: anywhere;
}
.detail-group-chevron {
    flex: none;
    color: var(--text-dim);
    transition: transform 0.18s ease;
}
.detail-group-chevron.open {
    transform: rotate(180deg);
}
.detail-group-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: 0 var(--space-4) var(--space-4);
    /* 卡内分隔线统一 var(--border)（同 CommunityView result-foot 口径） */
    border-top: 1px solid var(--border);
    padding-top: var(--space-3);
}
.detail-note {
    font-size: var(--text-xs);
}
.detail-loading {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-4) 0;
    font-size: var(--text-sm);
}
.detail-error {
    color: var(--danger);
    font-size: var(--text-sm);
    line-height: 1.7;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: flex-start;
}

/* 行样式与 CommunityView 下载弹窗 file-row 同族（同类功能同布局，UX「复用同一套布局」） */
.file-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--card-2);
    color: var(--text);
    font-family: inherit;
    text-align: left;
    cursor: pointer;
    transition:
        border-color 0.16s ease,
        background 0.16s ease;
}
.file-row:hover {
    background: var(--hover);
    border-color: var(--border-strong);
}
.file-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
}
.file-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 650;
    font-size: var(--text-sm);
    font-family: ui-monospace, Consolas, monospace;
}
.file-sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-xs);
    color: var(--text-dim);
}
.file-side {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: var(--space-3);
}
.file-meta {
    font-size: var(--text-xs);
    white-space: nowrap;
}
.file-dl {
    pointer-events: none;
}

/* ---------- modal 形态（沿用居中小窗） ---------- */
.community-project-modal {
    width: min(620px, calc(100vw - 32px));
    max-height: 85vh;
    overflow-y: auto;
    padding: 24px;
}
.project-heading {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
}
.project-heading > div {
    min-width: 0;
}
.project-heading .icon-btn {
    flex: none;
}
.project-platform {
    font-size: 11px;
}
.modal-title {
    margin: 6px 0 0;
    font-size: 20px;
    line-height: 1.5;
    overflow-wrap: anywhere;
}
.project-loading {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 28px 0;
    font-size: 13px;
}
.project-error {
    color: var(--danger);
    font-size: 13px;
    line-height: 1.7;
}
.project-summary {
    font-size: 14px;
    line-height: 1.8;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    margin: 20px 0;
}
.project-facts {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px 24px;
    margin: 20px 0;
    padding: 16px 0;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
}
.project-facts > div {
    min-width: 0;
}
.project-facts dt {
    font-size: 11px;
    color: var(--text-dim);
    margin-bottom: 5px;
}
.project-facts dd {
    margin: 0;
    font-size: 13px;
    line-height: 1.6;
    overflow-wrap: anywhere;
}
.project-categories {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
}
.project-categories .tag {
    font-size: 11px;
}
.modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 24px;
}
.project-source {
    margin-right: auto;
}
@media (max-width: 600px) {
    .community-project-modal {
        padding: 20px;
    }
    .project-facts {
        gap: 12px;
    }
    .modal-title {
        font-size: 18px;
    }
    .detail-intro-top {
        flex-direction: column;
    }
}
@media (prefers-reduced-motion: reduce) {
    .detail-group-chevron {
        transition: none;
    }
}
</style>
