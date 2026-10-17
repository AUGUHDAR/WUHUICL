<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import ContentSkeleton from '../../../components/ContentSkeleton.vue';
import CommunityResultCard from './CommunityResultCard.vue';
import { communityFileMatchesInstance } from '@shared/communityPolicy';
import type { CommunityKind, CommunityProjectReference, CommunityResult } from '@shared/types';

const props = defineProps<{
    results: CommunityResult[];
    kind: CommunityKind;
    loading: boolean;
    loadingMore: boolean;
    searched: boolean;
    loadError: string;
    warnings: string[];
    usesPagination: boolean;
    hasMore: boolean;
    currentPage: number;
    totalPages: number;
    totalResults: number;
    visiblePages: number[];
    favorites: Array<{ key: string }>;
    favoriteBusy: Set<string>;
}>();
const emit = defineEmits<{
    retry: [];
    loadMore: [];
    page: [page: number];
    download: [item: CommunityResult];
    favorite: [item: CommunityResult];
    source: [item: CommunityResult];
    mcmod: [item: CommunityResult];
}>();
const brokenIcons = ref(new Set<string>());
const moreSentinel = ref<HTMLElement | null>(null);
const listCard = ref<HTMLElement | null>(null);
let observer: IntersectionObserver | null = null;
const itemKey = (item: CommunityProjectReference) => `${item.source}:${item.projectId}`;
const isFavorite = (item: CommunityProjectReference) =>
    props.favorites.some((favorite) => favorite.key === itemKey(item));
const visiblePages = computed(() => props.visiblePages);

onMounted(() => {
    observer = new IntersectionObserver(
        (entries) => {
            if (
                !props.usesPagination &&
                entries.some((entry) => entry.isIntersecting) &&
                props.hasMore &&
                !props.loading &&
                !props.loadingMore
            )
                emit('loadMore');
        },
        { root: null, rootMargin: '240px', threshold: 0 }
    );
    watch(
        moreSentinel,
        (element) => {
            observer?.disconnect();
            if (element) observer?.observe(element);
        },
        { immediate: true }
    );
});
onUnmounted(() => observer?.disconnect());

async function goToPage(page: number) {
    if (props.loading || page < 1 || page > props.totalPages || page === props.currentPage) return;
    emit('page', page);
    await nextTick();
    listCard.value?.scrollIntoView({ block: 'start', behavior: 'smooth' });
}
</script>

<template>
    <div data-ui="CommunityView:5e55abba5e8a" ref="listCard" class="card list-card">
        <div
            data-ui="CommunityView:72423555b623"
            v-if="results.length && (loading || loadError)"
            class="status-strip"
            role="status"
        >
            {{ loading ? '正在更新条件，暂时显示上次结果…' : '更新失败，以下为上次结果：' + loadError
            }}<button
                data-ui="CommunityView:4ab4e45e43a0"
                v-if="loadError"
                class="btn btn-ghost btn-sm"
                @click="emit('retry')"
            >
                重试
            </button>
        </div>
        <p
            data-ui="CommunityView:fa2eef57b2fc"
            v-for="warning in warnings"
            :key="warning"
            class="search-warning"
        >
            {{ warning }}
        </p>
        <ContentSkeleton
            v-if="loading && !results.length"
            label="正在搜索社区资源…"
            :rows="6"
            retry
            @retry="emit('retry')"
        />
        <div data-ui="CommunityView:680f71022a3b" v-else-if="loadError && !results.length" class="empty">
            <span>搜索失败：{{ loadError }}</span>
            <button data-ui="CommunityView:dadfdac6acc0" class="btn btn-ghost btn-sm" @click="emit('retry')">
                重试
            </button>
        </div>
        <div data-ui="CommunityView:170233f26efe" v-else-if="!results.length" class="empty">
            <svg
                viewBox="0 0 24 24"
                width="40"
                height="40"
                fill="none"
                stroke="currentColor"
                stroke-width="1.6"
                stroke-linecap="round"
                stroke-linejoin="round"
            >
                <circle cx="12" cy="12" r="9" />
                <path d="M3 12h18" />
                <path d="M12 3a13.5 13.5 0 0 1 0 18" />
                <path d="M12 3a13.5 13.5 0 0 0 0 18" />
            </svg>
            <span>{{
                searched ? '没有找到匹配的资源，换个关键词或条件试试' : '输入关键词或选择条件开始搜索'
            }}</span>
        </div>
        <template v-else>
            <div
                data-ui="CommunityView:ea58bb908a86"
                class="result-list"
                :inert="loading || !!loadError"
                :aria-busy="loading"
            >
                <CommunityResultCard
                    v-for="item in results"
                    :key="itemKey(item)"
                    :item="item"
                    :kind="kind"
                    :favorite="isFavorite(item)"
                    :favorite-busy="favoriteBusy.has(itemKey(item))"
                    :broken-icon="brokenIcons.has(itemKey(item))"
                    @icon-error="brokenIcons = new Set([...brokenIcons, itemKey(item)])"
                    @source="emit('source', item)"
                    @mcmod="emit('mcmod', item)"
                    @favorite="emit('favorite', item)"
                    @download="emit('download', item)"
                />
            </div>
            <div
                data-ui="CommunityView:1833f0867a9c"
                v-if="!usesPagination && hasMore && !loading && !loadError"
                ref="moreSentinel"
                class="more-sentinel"
            ></div>
            <div data-ui="CommunityView:c18472307641" v-if="!usesPagination && hasMore" class="more-row">
                <button
                    data-ui="CommunityView:165371e35396"
                    class="btn btn-ghost"
                    :disabled="loadingMore"
                    @click="emit('loadMore')"
                >
                    <span data-ui="CommunityView:6fa61ac3821a" v-if="loadingMore" class="spin"></span>
                    {{ loadingMore ? '加载中…' : '加载更多' }}
                </button>
            </div>
        </template>
        <nav
            data-ui="CommunityView:d47a3ad6ba73"
            v-if="usesPagination && searched && totalPages > 1"
            class="pagination"
            aria-label="资源分页"
        >
            <span class="muted">共 {{ totalResults }} 项 · 第 {{ currentPage }} / {{ totalPages }} 页</span>
            <button
                data-ui="CommunityView:391d4872befc"
                class="btn btn-ghost btn-sm"
                :disabled="loading || currentPage <= 1"
                @click="goToPage(currentPage - 1)"
            >
                上一页
            </button>
            <button
                data-ui="CommunityView:79ac33e57fdc"
                v-for="page in visiblePages"
                :key="page"
                class="btn btn-sm"
                :class="page === currentPage ? 'btn-gold' : 'btn-ghost'"
                :aria-current="page === currentPage ? 'page' : undefined"
                :disabled="loading"
                @click="goToPage(page)"
            >
                {{ page }}
            </button>
            <button
                data-ui="CommunityView:2f9290893a14"
                class="btn btn-ghost btn-sm"
                :disabled="loading || currentPage >= totalPages"
                @click="goToPage(currentPage + 1)"
            >
                下一页
            </button>
        </nav>
    </div>
</template>

<style scoped>
.list-card {
    display: grid;
    gap: 12px;
    padding: 14px;
}
.status-strip,
.search-warning {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    font-size: 12px;
}
.status-strip {
    color: var(--text-dim);
}
.search-warning {
    margin: 0;
    color: var(--warning, #d97706);
}
.result-list {
    display: grid;
    gap: 10px;
}
.result-list[inert] {
    opacity: 0.72;
}
.more-sentinel {
    height: 1px;
}
.more-row {
    display: flex;
    justify-content: center;
}
.pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    flex-wrap: wrap;
    padding-top: 8px;
}
.pagination .muted {
    margin-right: auto;
    font-size: 11px;
}
</style>
