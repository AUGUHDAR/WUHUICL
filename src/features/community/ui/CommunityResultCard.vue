<script setup lang="ts">
import type { CommunityKind, CommunityResult } from '@shared/types';

defineProps<{
    item: CommunityResult;
    kind: CommunityKind;
    favorite: boolean;
    favoriteBusy: boolean;
    brokenIcon: boolean;
}>();
const emit = defineEmits<{
    iconError: [];
    source: [];
    mcmod: [];
    favorite: [];
    download: [];
}>();

const fmtDownloads = (value: number): string => {
    if (value >= 1e8) return (value / 1e8).toFixed(1) + ' 亿';
    if (value >= 1e4) return (value / 1e4).toFixed(1) + ' 万';
    return String(value);
};
const fmtDate = (value: string): string => {
    if (!value) return '—';
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString('zh-CN');
};
</script>

<template>
    <div data-ui="CommunityView:0d40e298da83" class="result-card">
        <div data-ui="CommunityView:491b5f81c088" class="result-top">
            <div data-ui="CommunityView:00f58a1819fa" class="result-icon">
                <img
                    data-ui="CommunityView:830997d3ecb7"
                    v-if="item.iconUrl && !brokenIcon"
                    :src="item.iconUrl"
                    alt=""
                    loading="lazy"
                    @error="emit('iconError')"
                />
                <span data-ui="CommunityView:a6c25e1efd38" v-else class="icon-placeholder">{{
                    (item.title || '?').charAt(0).toUpperCase()
                }}</span>
            </div>
            <div data-ui="CommunityView:d33af84ab969" class="result-head">
                <strong class="result-title" tabindex="0" :title="item.title">{{
                    item.title
                }}</strong>
                <span
                    data-ui="CommunityView:6f93851d6071"
                    class="tag"
                    :class="item.source === 'modrinth' ? 'tag-success' : 'tag-cf'"
                >
                    来源：{{ item.source === 'modrinth' ? 'Modrinth' : 'CurseForge' }}
                </span>
                <span
                    data-ui="CommunityView:ad4b7d43f757"
                    v-if="item.author"
                    class="muted result-author"
                    >{{ item.author }}</span
                >
            </div>
        </div>
        <p data-ui="CommunityView:2c1a48d38ee1" class="result-desc" :title="item.description">
            {{ item.description || '暂无简介' }}
        </p>
        <div data-ui="CommunityView:b2d346eb1503" class="result-meta muted">
            <span>下载量 {{ fmtDownloads(item.downloads) }}</span>
            <span data-ui="CommunityView:e69c45508563" class="meta-dot">·</span>
            <span>更新于 {{ fmtDate(item.updatedAt) }}</span>
        </div>
        <div data-ui="CommunityView:9195d6b103b6" class="result-foot">
            <div data-ui="CommunityView:22746f0aa9cd" class="result-links">
                <button
                    data-ui="CommunityView:7c3533e67ba3"
                    class="icon-btn"
                    :title="`打开 ${item.source === 'modrinth' ? 'Modrinth' : 'CurseForge'} 源页面（查看完整介绍）`"
                    @click="emit('source')"
                >
                    <svg
                        viewBox="0 0 24 24"
                        width="14"
                        height="14"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
                        <path d="M15 3h6v6" />
                        <path d="M10 14 21 3" />
                    </svg>
                </button>
                <button
                    data-ui="CommunityView:0c667bb024b0"
                    class="icon-btn"
                    title="在 MC 百科查看介绍与教程"
                    @click="emit('mcmod')"
                >
                    <svg
                        viewBox="0 0 24 24"
                        width="14"
                        height="14"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <path
                            d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20"
                        />
                    </svg>
                </button>
                <button
                    v-if="kind === 'mod'"
                    class="icon-btn result-favorite"
                    :class="{ active: favorite }"
                    :aria-label="`${favorite ? '取消收藏' : '收藏'} ${item.title}`"
                    :aria-pressed="favorite"
                    :title="favorite ? '取消收藏模组' : '收藏模组'"
                    :disabled="favoriteBusy"
                    @click.stop="emit('favorite')"
                >
                    <svg
                        viewBox="0 0 24 24"
                        width="16"
                        height="16"
                        :fill="favorite ? 'currentColor' : 'none'"
                        stroke="currentColor"
                        stroke-width="1.7"
                        stroke-linejoin="round"
                    >
                        <path
                            d="m12 3 2.8 5.7 6.3.9-4.6 4.4 1.1 6.3-5.6-3-5.6 3 1.1-6.3L3 9.6l6.2-.9Z"
                        />
                    </svg>
                </button>
            </div>
            <button
                data-ui="CommunityView:6d17d47c8729"
                class="btn btn-gold btn-sm result-dl"
                @click="emit('download')"
            >
                <svg
                    viewBox="0 0 24 24"
                    width="13"
                    height="13"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <path d="M12 3v11" />
                    <path d="m7 10 5 5 5-5" />
                    <path d="M4 21h16" />
                </svg>
                下载
            </button>
        </div>
    </div>
</template>

<style scoped>
.result-card {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 10px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--card);
    transition: border-color 160ms;
}
.result-card:hover {
    border-color: var(--border-strong);
}
.result-top {
    display: flex;
    align-items: center;
    gap: 12px;
}
.result-icon {
    display: grid;
    flex: none;
    width: 48px;
    height: 48px;
    place-items: center;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--card-2);
}
.result-icon img {
    width: 100%;
    height: 100%;
    object-fit: contain;
}
.icon-placeholder {
    color: var(--accent-2);
    font-size: 20px;
    font-weight: 700;
}
.result-head {
    display: flex;
    flex: 1;
    min-width: 0;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
}
.result-title {
    min-width: 0;
    overflow: hidden;
    font-size: 14px;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.result-author {
    font-size: 11px;
}
.result-desc {
    display: -webkit-box;
    overflow: hidden;
    margin: 0;
    color: var(--text-dim);
    font-size: 12px;
    line-height: 1.65;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
}
.result-meta {
    display: flex;
    gap: 5px;
    flex-wrap: wrap;
    font-size: 11px;
}
.result-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
}
.result-links {
    display: flex;
    gap: 2px;
}
.result-favorite.active {
    color: var(--accent-2);
}
.tag-cf {
    color: #f7a063;
    background: rgba(247, 160, 99, 0.12);
}
</style>
