<script setup lang="ts">
/** 大厅：浏览公开房间；无密码房间点「加入」直接进房，有密码才带码去加入页。 */
import { onMounted, ref } from 'vue';
import { natLabel } from '../../../../shared/voxlinkNat';
import {
    call,
    categories,
    join,
    loadCategories,
    pendingJoinCode,
    pendingJoinError,
    refresh,
    section,
    type VlRoomCard
} from './useVoxlink';
import './voxlink.css';

const rooms = ref<VlRoomCard[]>([]);
const page = ref(1);
const size = 50;
const search = ref('');
const category = ref('');
const loading = ref(false);
const error = ref('');
/** 正在直加的房号（卡片按钮转忙碌），同一时刻只发一次加入 */
const joiningCode = ref('');

function pickList(data: unknown): VlRoomCard[] {
    if (Array.isArray(data)) return data as VlRoomCard[];
    const obj = (data || {}) as Record<string, unknown>;
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
        error.value = r.message || '大厅加载失败，点击「刷新大厅」重试';
        rooms.value = [];
    } else {
        rooms.value = pickList(r.data);
    }
    loading.value = false;
}

function filterBy(value: string) {
    category.value = category.value === value ? '' : value;
    void load(1);
}

function categoryLabel(room: VlRoomCard): string {
    if (!room.category) return '一起游玩';
    return categories.value[room.category] || room.category;
}

function versionLine(room: VlRoomCard): string {
    return [room.gameVersion, room.loader].filter(Boolean).join(' · ') || '版本未标注';
}

async function goJoin(room: VlRoomCard) {
    if (joiningCode.value) return;
    if (room.hasPassword) {
        // 有密码：直接跳加入页，房号已填好，只差密码
        pendingJoinCode.value = room.code;
        pendingJoinError.value = '';
        section.value = 'join';
        return;
    }
    joiningCode.value = room.code;
    const r = await join(room.code, '', 'gate');
    joiningCode.value = '';
    if (!r.ok) {
        // 直加没成（可能实际要密码）：带码转加入页，把原因带上
        pendingJoinCode.value = room.code;
        pendingJoinError.value = r.message || '加入没有完成，在这里再试一次';
        section.value = 'join';
        return;
    }
    section.value = 'room';
    await refresh();
}

onMounted(() => {
    void load(1);
    void loadCategories();
});
</script>

<template>
    <div class="vl-card" data-ui="voxlink:lobby">
        <div class="vl-card-head">
            <div>
                <h2>公开房间</h2>
                <p>别人开着的房间会列在这里，加入前可以先看版本和加载器对不对得上。</p>
            </div>
            <div class="vl-row">
                <button class="btn btn-ghost btn-sm" :disabled="loading" @click="load()">
                    {{ loading ? '刷新中…' : '刷新大厅' }}
                </button>
            </div>
        </div>

        <div class="vl-row">
            <input
                v-model="search"
                class="input"
                style="max-width: 260px"
                placeholder="搜索房间名，回车查询"
                @keyup.enter="load(1)"
            />
            <button
                v-for="(label, value) in categories"
                :key="value"
                class="vl-pill"
                :class="{ ok: category === value }"
                @click="filterBy(value)"
            >
                {{ label }}
            </button>
        </div>

        <div v-if="error" class="vl-error">{{ error }}</div>

        <div v-else-if="loading && !rooms.length" class="vl-empty">正在寻找公开房间…</div>

        <div v-else-if="!rooms.length" class="vl-empty">暂时没有公开房间，创建一个邀请好友吧。</div>

        <div v-else class="vl-grid" style="margin-top: 14px">
            <div v-for="room in rooms" :key="room.code" class="vl-room">
                <div class="vl-room-name">{{ room.name || '未命名房间' }}</div>
                <div class="vl-room-meta">
                    <span class="vl-pill">{{ categoryLabel(room) }}</span>
                    <span v-if="room.clientTag" class="vl-pill">{{ room.clientTag.toUpperCase() }} 房间</span>
                    <span v-if="room.hasPassword" class="vl-pill">有密码</span>
                </div>
                <div class="vl-code">{{ room.code }}</div>
                <div class="vl-players">
                    {{ versionLine(room) }} · {{ room.currentPlayers ?? '?' }}/{{ room.maxPlayers ?? '?' }} 人 · NAT
                    {{ natLabel('UNKNOWN', room.natType) }}
                </div>
                <button class="btn btn-gold btn-sm" :disabled="!!joiningCode" @click="goJoin(room)">
                    {{ joiningCode === room.code ? '正在加入…' : room.hasPassword ? '输入密码加入' : '加入' }}
                </button>
            </div>
        </div>

        <div v-if="rooms.length >= size" class="vl-actions">
            <button class="btn btn-ghost btn-sm" :disabled="page <= 1 || loading" @click="load(page - 1)">上一页</button>
            <span class="vl-hint">第 {{ page }} 页</span>
            <button class="btn btn-ghost btn-sm" :disabled="loading" @click="load(page + 1)">下一页</button>
        </div>
    </div>
</template>
