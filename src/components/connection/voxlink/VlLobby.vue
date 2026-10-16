<script setup lang="ts">
/** 大厅：浏览公开房间，选中后带去加入页。 */
import { onMounted, ref } from 'vue';
import { call, pendingJoinCode, section, type VlRoomCard } from './useVoxlink';
import './voxlink.css';

const rooms = ref<VlRoomCard[]>([]);
const total = ref(0);
const page = ref(1);
const size = 24;
const search = ref('');
const category = ref('');
const loading = ref(false);
const error = ref('');

function pickList(data: unknown): VlRoomCard[] {
    if (Array.isArray(data)) return data as VlRoomCard[];
    const obj = (data || {}) as Record<string, unknown>;
    total.value = Number(obj.total || obj.count || 0);
    for (const key of ['rooms', 'items', 'list']) {
        if (Array.isArray(obj[key])) return obj[key] as VlRoomCard[];
    }
    return [];
}

async function load(target = page.value) {
    loading.value = true;
    error.value = '';
    page.value = target;
    const r = await call<Record<string, unknown>>('vlcli:lobby', {
        page: target,
        size,
        search: search.value.trim() || undefined,
        category: category.value || undefined
    });
    if (!r.ok) {
        error.value = r.message || '大厅没取回来';
        rooms.value = [];
    } else {
        rooms.value = pickList(r.data);
    }
    loading.value = false;
}

function goJoin(room: VlRoomCard) {
    pendingJoinCode.value = room.code;
    section.value = 'join';
}

onMounted(() => {
    void load(1);
});

const pages = () => Math.max(1, Math.ceil((total.value || rooms.value.length) / size));
</script>

<template>
    <div class="vl-card" data-ui="voxlink:lobby">
        <div class="vl-card-head">
            <div>
                <h2>公开房间</h2>
                <p>别人开着的房间会出现在这里。加入前可以先看版本和加载器对不对得上。</p>
            </div>
            <button class="btn btn-ghost btn-sm" :disabled="loading" @click="load()">
                {{ loading ? '刷新中…' : '刷新' }}
            </button>
        </div>

        <div class="vl-row">
            <input
                v-model="search"
                class="input"
                placeholder="按房间名搜索"
                style="max-width: 260px"
                @keyup.enter="load(1)"
            />
            <input
                v-model="category"
                class="input"
                placeholder="分类，例如 survival"
                style="max-width: 180px"
                @keyup.enter="load(1)"
            />
            <button class="btn btn-ghost" @click="load(1)">查询</button>
        </div>

        <div v-if="error" class="vl-error">{{ error }}</div>

        <div v-else-if="loading && !rooms.length" class="vl-empty">正在加载大厅…</div>

        <div v-else-if="!rooms.length" class="vl-empty">
            现在没有公开房间。可以直接建一个，或者让朋友把 6 位房号发给你。
        </div>

        <div v-else class="vl-grid" style="margin-top: 14px">
            <div v-for="room in rooms" :key="room.code" class="vl-room">
                <div class="vl-room-name">{{ room.name || '未命名房间' }}</div>
                <div class="vl-room-meta">
                    <span class="vl-code">{{ room.code }}</span>
                    <span v-if="room.hasPassword" class="vl-pill">有密码</span>
                    <span v-if="room.loader" class="vl-pill">{{ room.loader }}</span>
                    <span v-if="room.gameVersion" class="vl-pill">{{ room.gameVersion }}</span>
                    <span v-if="room.natType" class="vl-pill nat">{{ room.natType }}</span>
                </div>
                <div class="vl-players">
                    {{ room.currentPlayers ?? '?' }} / {{ room.maxPlayers ?? '?' }} 人
                    <span v-if="room.clientTag"> · 来自 {{ room.clientTag }}</span>
                </div>
                <button class="btn btn-sm" @click="goJoin(room)">加入</button>
            </div>
        </div>

        <div v-if="rooms.length" class="vl-actions">
            <button class="btn btn-ghost btn-sm" :disabled="page <= 1 || loading" @click="load(page - 1)">
                上一页
            </button>
            <span class="vl-hint">第 {{ page }} / {{ pages() }} 页</span>
            <button class="btn btn-ghost btn-sm" :disabled="page >= pages() || loading" @click="load(page + 1)">
                下一页
            </button>
        </div>
    </div>
</template>
