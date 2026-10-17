<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import SelectMenu from '../../../components/SelectMenu.vue';
import CommunityVersionFilter from './CommunityVersionFilter.vue';
import type {
    CommunityKind,
    CommunitySource,
    InstalledVersion,
    LoaderName
} from '@shared/types';
import type {
    CommunityVersionSelection,
    CommunityVersionSource
} from '../data/communityVersionSelection';
import { instanceKey } from '@shared/modCompatibility';

const props = defineProps<{
    query: {
        keyword: string;
        kind: CommunityKind;
        source: 'all' | CommunitySource;
        mcVersion: string;
        loader: '' | LoaderName;
        sort: 'relevance' | 'downloads' | 'newest';
    };
    versionSelection: CommunityVersionSelection;
    installedVersionOptions: Array<{ value: string; label: string; description: string }>;
    manifestVersions: string[];
    manifestLoading: boolean;
    versionSummary: string;
    loaderSummary: string;
    supportsLoader: boolean;
    manualLoaderMismatch: boolean;
    filterLoader?: string;
    currentInstance?: InstalledVersion;
    loading: boolean;
}>();
const emit = defineEmits<{
    search: [];
    reset: [];
    filter: [];
    versionSource: [source: CommunityVersionSource];
    versionChanged: [];
    loaderChanged: [];
    instance: [id: string];
    currentInstance: [];
    instanceLoader: [];
}>();

const kindTabs: Array<{ value: CommunityKind; label: string }> = [
    { value: 'mod', label: 'Mod' },
    { value: 'modpack', label: '整合包' },
    { value: 'resourcepack', label: '资源包' },
    { value: 'shader', label: '光影包' },
    { value: 'datapack', label: '数据包' }
];
const sourceOptions: Array<{ value: 'all' | CommunitySource; label: string }> = [
    { value: 'all', label: '全部来源' },
    { value: 'modrinth', label: 'Modrinth' },
    { value: 'curseforge', label: 'CurseForge' }
];
const loaderOptions: Array<{ value: '' | LoaderName; label: string }> = [
    { value: '', label: '全部加载器' },
    { value: 'forge', label: 'Forge' },
    { value: 'fabric', label: 'Fabric' },
    { value: 'quilt', label: 'Quilt' },
    { value: 'neoforge', label: 'NeoForge' }
];
const sortOptions = [
    { value: 'relevance', label: '相关度' },
    { value: 'downloads', label: '最多下载' },
    { value: 'newest', label: '最新发布' }
];
const kindCapsules = ref<HTMLElement | null>(null);
const kindBlob = reactive({ left: 0, top: 0, width: 0, height: 0, on: false });
function updateKindBlob() {
    const root = kindCapsules.value;
    if (!root) return;
    const active = root.querySelector<HTMLElement>(`.capsule[data-kind="${props.query.kind}"]`);
    if (!active) return;
    kindBlob.left = active.offsetLeft;
    kindBlob.top = active.offsetTop;
    kindBlob.width = active.offsetWidth;
    kindBlob.height = active.offsetHeight;
    kindBlob.on = true;
}
let kindBlobObserver: ResizeObserver | null = null;
onMounted(() => {
    updateKindBlob();
    setTimeout(updateKindBlob, 200);
    kindBlobObserver = new ResizeObserver(updateKindBlob);
    if (kindCapsules.value) kindBlobObserver.observe(kindCapsules.value);
});
onUnmounted(() => kindBlobObserver?.disconnect());
watch(
    () => props.query.kind,
    () => setTimeout(updateKindBlob)
);
const kindBlobStyle = computed(() => ({
    left: kindBlob.left + 'px',
    top: kindBlob.top + 'px',
    width: kindBlob.width + 'px',
    height: kindBlob.height + 'px',
    opacity: kindBlob.on ? 1 : 0
}));

function setKind(kind: CommunityKind) {
    props.query.kind = kind;
    emit('filter');
}
function setInstance(id: string) {
    emit('instance', id);
}
</script>

<template>
    <div data-ui="CommunityView:5551ade589f2" class="card search-card">
        <div data-ui="CommunityView:cc7ad8d814fc" ref="kindCapsules" class="kind-capsules">
            <span
                data-ui="CommunityView:911176084f18"
                class="capsule-blob"
                :style="kindBlobStyle"
                aria-hidden="true"
            ></span>
            <button
                data-ui="CommunityView:9275c1ee8bbc"
                v-for="kind in kindTabs"
                :key="kind.value"
                class="capsule"
                :data-kind="kind.value"
                :class="{ active: query.kind === kind.value }"
                @click="setKind(kind.value)"
            >
                {{ kind.label }}
            </button>
        </div>
        <section class="version-filter-panel" aria-label="Minecraft 版本筛选">
            <div class="version-filter-heading">
                <strong>适用游戏版本</strong
                ><span class="muted"
                    >{{ versionSummary
                    }}<template v-if="supportsLoader"> · {{ loaderSummary }}</template></span
                >
            </div>
            <div class="version-source-options" role="group" aria-label="版本选择来源">
                <button
                    type="button"
                    class="version-source-option"
                    data-ui="community:versions-all"
                    :class="{ active: versionSelection.source === 'all' }"
                    :aria-pressed="versionSelection.source === 'all'"
                    @click="emit('versionSource', 'all')"
                >
                    全部版本
                </button>
                <button
                    type="button"
                    class="version-source-option"
                    data-ui="community:versions-installed"
                    :class="{ active: versionSelection.source === 'installed' }"
                    :aria-pressed="versionSelection.source === 'installed'"
                    @click="emit('versionSource', 'installed')"
                >
                    已安装版本<span>{{ installedVersionOptions.length }}</span>
                </button>
                <button
                    type="button"
                    class="version-source-option"
                    data-ui="community:versions-custom"
                    :class="{ active: versionSelection.source === 'custom' }"
                    :aria-pressed="versionSelection.source === 'custom'"
                    @click="emit('versionSource', 'custom')"
                >
                    自定义版本
                </button>
            </div>
            <div
                v-if="versionSelection.source === 'installed'"
                class="version-filter-detail"
                data-ui="CommunityView:f7acd66aeb10"
            >
                <SelectMenu
                    v-if="installedVersionOptions.length"
                    class="instance-filter"
                    aria-label="选择已安装版本"
                    :model-value="versionSelection.instance"
                    placeholder="选择已安装实例以筛选…"
                    :options="installedVersionOptions"
                    @change="setInstance"
                />
                <p v-else class="muted">暂无可识别的已安装版本；可使用自定义版本。</p>
                <button
                    data-ui="CommunityView:49df26abb0c5"
                    class="btn btn-ghost btn-sm"
                    :disabled="!currentInstance || !currentInstance.mcVersion"
                    @click="emit('currentInstance')"
                >
                    使用当前实例
                </button>
            </div>
            <div
                v-if="versionSelection.source === 'custom'"
                class="version-filter-detail custom-version-detail"
                data-ui="community:custom-version"
            >
                <CommunityVersionFilter
                    :model-value="query.mcVersion"
                    :versions="manifestVersions"
                    :loading="manifestLoading"
                    @update:model-value="(value) => (query.mcVersion = value)"
                    @change="emit('versionChanged')"
                />
                <p class="muted">
                    从列表选择，或输入完整版本后按 Enter；快照和自定义版本按原值查询。
                </p>
            </div>
            <div
                v-if="manualLoaderMismatch && supportsLoader"
                class="version-loader-note"
                role="status"
            >
                已保留手选的 {{ loaderSummary }}；该实例使用 {{ filterLoader || '原版' }}。<button
                    type="button"
                    class="btn btn-ghost btn-sm"
                    @click="emit('instanceLoader')"
                >
                    使用实例加载器
                </button>
            </div>
        </section>
        <div data-ui="CommunityView:d516b82f0eb8" class="search-row">
            <input
                data-ui="CommunityView:bc0450fd9c8f"
                v-model="query.keyword"
                class="input"
                :placeholder="
                    query.kind === 'mod'
                        ? '输入 MOD 中文名、常用别名或英文名，回车搜索…'
                        : '输入资源名称，回车搜索…'
                "
                @keyup.enter="emit('search')"
            />
            <button
                data-ui="CommunityView:ce39174e4563"
                class="btn btn-gold search-btn"
                :disabled="loading"
                @click="emit('search')"
            >
                <span data-ui="CommunityView:bb1887897e8c" v-if="loading" class="spin"></span>
                <svg
                    v-else
                    viewBox="0 0 24 24"
                    width="14"
                    height="14"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <circle cx="11" cy="11" r="7" />
                    <path d="m21 21-4.3-4.3" />
                </svg>
                搜索
            </button>
            <button
                data-ui="CommunityView:15aecd36844d"
                class="btn btn-ghost"
                :disabled="loading"
                @click="emit('reset')"
            >
                重置条件
            </button>
        </div>
        <div class="filter-row">
            <SelectMenu
                aria-label="资源来源"
                v-model="query.source"
                class="filter-select"
                :options="sourceOptions"
                @change="emit('filter')"
            />
            <SelectMenu
                aria-label="加载器"
                v-if="supportsLoader"
                v-model="query.loader"
                class="filter-select"
                :options="loaderOptions"
                @change="emit('loaderChanged')"
            />
            <SelectMenu
                aria-label="排序"
                v-model="query.sort"
                class="filter-select"
                :options="sortOptions"
                @change="emit('filter')"
            />
        </div>
    </div>
</template>

<style scoped>
.search-card {
    padding: 16px;
}
.kind-capsules {
    position: relative;
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    padding-bottom: 12px;
}
.capsule-blob {
    position: absolute;
    z-index: 0;
    border-radius: var(--radius-sm);
    background: var(--accent-soft);
    transition:
        left 0.2s ease,
        top 0.2s ease,
        width 0.2s ease,
        height 0.2s ease,
        opacity 0.15s ease;
}
.capsule {
    position: relative;
    z-index: 1;
    min-height: 32px;
    padding: 6px 14px;
    border: 0;
    background: transparent;
    color: var(--text-dim);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
}
.capsule.active {
    color: var(--accent-2);
    font-weight: 600;
}
.version-filter-panel {
    display: grid;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--card-2);
}
.version-filter-heading {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
    font-size: 13px;
}
.version-filter-heading .muted {
    font-size: 11px;
}
.version-source-options {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
}
.version-source-option {
    min-height: 30px;
    padding: 5px 11px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: transparent;
    color: var(--text-dim);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
}
.version-source-option.active {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent-2);
}
.version-source-option span {
    margin-left: 4px;
}
.version-filter-detail {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
}
.custom-version-detail {
    align-items: flex-start;
}
.custom-version-detail .muted {
    flex: 1;
    min-width: 220px;
    font-size: 11px;
    line-height: 1.6;
}
.version-loader-note {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    color: var(--text-dim);
    font-size: 11px;
}
.search-row {
    display: flex;
    gap: 8px;
    margin-top: 14px;
}
.search-row .input {
    flex: 1;
    min-width: 0;
}
.filter-row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 10px;
}
.filter-row > * {
    flex: 1;
    min-width: 150px;
}
@media (max-width: 700px) {
    .search-row {
        align-items: stretch;
        flex-direction: column;
    }
}
</style>
