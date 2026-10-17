<script setup lang="ts">
import { computed } from 'vue';
import MarqueeText from '../../../components/MarqueeText.vue';
import SelectMenu from '../../../components/SelectMenu.vue';
import { usesCommunityLoader } from '@shared/communityPolicy';
import { instanceKey } from '@shared/modCompatibility';
import type {
    CommunityFile,
    CommunityKind,
    CommunityProjectReference,
    InstalledVersion,
    LoaderName
} from '@shared/types';

const props = defineProps<{
    item: CommunityProjectReference | null;
    kind: CommunityKind;
    files: CommunityFile[];
    loading: boolean;
    error: string;
    fileId: string;
    versionId: string;
    mcVersion: string;
    loader: '' | LoaderName;
    targetInstances: InstalledVersion[];
    manifestVersions: string[];
    favorite: boolean;
    favoriteBusy: boolean;
}>();
const emit = defineEmits<{
    close: [];
    links: [];
    mcmod: [];
    favorite: [];
    loadFiles: [];
    updateMcVersion: [value: string];
    updateLoader: [value: '' | LoaderName];
    updateVersionId: [value: string];
    confirm: [file: CommunityFile, target?: InstalledVersion];
}>();
const loaderOptions: Array<{ value: '' | LoaderName; label: string }> = [
    { value: '', label: '全部加载器' },
    { value: 'forge', label: 'Forge' },
    { value: 'fabric', label: 'Fabric' },
    { value: 'quilt', label: 'Quilt' },
    { value: 'neoforge', label: 'NeoForge' }
];
const isModpack = computed(() => props.kind === 'modpack');
const selectedFile = computed(() => props.files.find((file) => file.fileId === props.fileId) ?? null);
const canConfirm = computed(
    () =>
        !!selectedFile.value &&
        !props.loading &&
        (isModpack.value || props.targetInstances.some((v) => instanceKey(v) === props.versionId))
);
const releaseTagClass = (type: CommunityFile['releaseType']) =>
    type === 'beta' ? 'tag-cyan' : type === 'alpha' ? 'tag-danger' : 'tag-gold';
const releaseText: Record<CommunityFile['releaseType'], string> = {
    release: '正式版',
    beta: 'Beta',
    alpha: 'Alpha'
};
const fmtDate = (value: string): string => {
    if (!value) return '—';
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString('zh-CN');
};
const fmtSize = (bytes: number): string => {
    if (!bytes || bytes <= 0) return '—';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
};
function setFile(fileId: string) {
    emit('updateVersionId', fileId);
}
</script>

<template>
    <Teleport to="body">
        <div
            data-ui="CommunityView:ef88acc39749"
            v-if="item"
            class="modal-mask"
            @pointerdown.self="emit('close')"
        >
            <div data-ui="CommunityView:6904c547ed30" class="modal download-modal">
                <h3 data-ui="CommunityView:7b81ed690844" class="modal-title">
                    <MarqueeText :text="'下载 ' + item.title" />
                </h3>
                <div data-ui="CommunityView:a84b1e456827" class="modal-links">
                    <button
                        v-if="kind === 'mod'"
                        class="btn btn-ghost btn-sm"
                        :disabled="favoriteBusy"
                        :aria-pressed="favorite"
                        @click="emit('favorite')"
                    >
                        {{ favorite ? '★ 已收藏' : '☆ 收藏模组' }}
                    </button>
                    <button
                        data-ui="CommunityView:6fabba70cd3a"
                        class="btn btn-ghost btn-sm"
                        @click="emit('links')"
                    >
                        {{ item.source === 'modrinth' ? 'Modrinth 源页面' : 'CurseForge 源页面' }}
                    </button>
                    <button
                        data-ui="CommunityView:03eb0c52ad3e"
                        class="btn btn-ghost btn-sm"
                        @click="emit('mcmod')"
                    >
                        MC 百科介绍
                    </button>
                </div>
                <div class="filter-row">
                    <label data-ui="CommunityView:9b7baa1d1a72" class="modal-field"
                        >Minecraft 版本<input
                            data-ui="CommunityView:75b46121b566"
                            :value="mcVersion"
                            class="input"
                            list="mod-minecraft-versions"
                            placeholder="全部版本"
                            @input="
                                emit(
                                    'updateMcVersion',
                                    ($event.target as HTMLInputElement).value
                                );
                                emit('loadFiles');
                            "
                    /></label>
                    <label
                        data-ui="CommunityView:73416dae43e4"
                        v-if="usesCommunityLoader(kind)"
                        class="modal-field"
                        >Loader<SelectMenu
                            :model-value="loader"
                            :options="loaderOptions"
                            @update:model-value="
                                (value) => {
                                    emit('updateLoader', value as LoaderName | '');
                                    emit('loadFiles');
                                }
                            "
                    /></label>
                    <datalist data-ui="CommunityView:3ea5bc9c8890" id="mod-minecraft-versions">
                        <option v-for="version in manifestVersions" :key="version" :value="version" />
                    </datalist>
                </div>
                <p class="modal-label">选择文件版本</p>
                <div data-ui="CommunityView:8f95d8f66a15" v-if="loading" class="files-loading">
                    <span data-ui="CommunityView:8941adbc1d4f" class="spin"></span>
                    <span class="muted">正在获取文件列表…</span>
                </div>
                <template v-else>
                    <div data-ui="CommunityView:db1154820afe" v-if="files.length" class="file-list">
                        <button
                            data-ui="CommunityView:ceffac990295"
                            v-for="file in files"
                            :key="file.fileId"
                            class="file-row"
                            :class="{ active: fileId === file.fileId }"
                            @click="setFile(file.fileId)"
                        >
                            <span data-ui="CommunityView:05bff2e14ac9" class="file-main">
                                <span
                                    data-ui="CommunityView:3906a840cd50"
                                    class="file-name"
                                    :title="file.fileName"
                                    >{{ file.fileName }}</span
                                >
                                <span data-ui="CommunityView:a7eecc7757d6" class="file-sub"
                                    >版本 {{ file.version }} · MC {{ file.gameVersions.join(' / ')
                                    }}<template
                                        v-if="usesCommunityLoader(kind) && file.loaders.length"
                                    >
                                        · {{ file.loaders.join(' / ') }}</template
                                    ></span
                                >
                            </span>
                            <span data-ui="CommunityView:9584cb689677" class="file-side">
                                <span
                                    data-ui="CommunityView:44e7a728a5f3"
                                    class="tag"
                                    :class="releaseTagClass(file.releaseType)"
                                    >{{ releaseText[file.releaseType] }}</span
                                >
                                <span data-ui="CommunityView:32e67d25368a" class="muted file-meta"
                                    >{{ fmtDate(file.date) }} · {{ fmtSize(file.size) }}</span
                                >
                            </span>
                        </button>
                    </div>
                    <p data-ui="CommunityView:579adeba801a" v-if="error" class="files-error">
                        {{ error }}
                    </p>
                </template>
                <template v-if="!isModpack">
                    <p class="modal-label">下载到版本</p>
                    <SelectMenu
                        aria-label="下载目标实例"
                        v-if="targetInstances.length"
                        :model-value="versionId"
                        :options="
                            targetInstances.map((version) => ({
                                value: instanceKey(version),
                                label:
                                    version.id +
                                    ' · ' +
                                    version.mcVersion +
                                    ' / ' +
                                    (version.loader || '纯净版') +
                                    ' · ' +
                                    version.folder
                            }))
                        "
                            @update:model-value="(value) => emit('updateVersionId', value)"
                    />
                    <p data-ui="CommunityView:ab12acbb18fe" v-else class="files-error">
                        没有与所选文件兼容的已安装实例；可调整文件筛选，或在游戏版本页安装。
                    </p>
                </template>
                <p data-ui="CommunityView:a8e08b82f315" v-else class="muted pack-tip">
                    整合包将下载后自动创建独立实例并安装
                </p>
                <div data-ui="CommunityView:2356b94bbc0d" class="modal-actions">
                    <button data-ui="CommunityView:989d28842ec5" class="btn btn-ghost" @click="emit('close')">
                        取消
                    </button>
                    <button
                        data-ui="CommunityView:cade5c4fc83a"
                        class="btn btn-gold"
                        :disabled="!canConfirm"
                        @click="
                            selectedFile &&
                            emit(
                                'confirm',
                                selectedFile,
                                targetInstances.find((v) => instanceKey(v) === versionId)
                            )
                        "
                    >
                        {{ kind === 'mod' ? '加入队列并检测' : '确认下载' }}
                    </button>
                </div>
            </div>
        </div>
    </Teleport>
</template>

<style scoped>
.download-modal {
    width: min(760px, calc(100vw - 32px));
    max-height: 85vh;
    overflow-y: auto;
    padding: 24px;
}
.modal-title {
    margin: 0 0 12px;
    font-size: 20px;
}
.modal-links,
.filter-row,
.modal-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
}
.filter-row {
    margin-top: 14px;
}
.modal-field {
    display: flex;
    flex: 1;
    min-width: 160px;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
}
.modal-label {
    margin: 18px 0 8px;
    font-size: 13px;
    font-weight: 600;
}
.files-loading {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 20px 0;
}
.file-list {
    display: grid;
    max-height: 300px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
}
.file-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border: 0;
    border-bottom: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
}
.file-row:last-child {
    border-bottom: 0;
}
.file-row.active {
    background: var(--accent-soft);
}
.file-main {
    display: flex;
    flex: 1;
    min-width: 0;
    flex-direction: column;
    gap: 4px;
}
.file-name {
    overflow: hidden;
    font-size: 12px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.file-sub,
.file-meta {
    overflow: hidden;
    color: var(--text-dim);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.file-side {
    display: flex;
    align-items: flex-end;
    flex-direction: column;
    gap: 4px;
}
.files-error {
    color: var(--danger);
    font-size: 12px;
    line-height: 1.6;
}
.pack-tip {
    font-size: 12px;
}
.modal-actions {
    justify-content: flex-end;
    margin-top: 20px;
}
</style>
