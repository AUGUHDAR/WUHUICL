<script setup lang="ts">
import { computed, ref } from 'vue';
import type { ServerEntry, ServerPingResult, InstalledVersion } from '@shared/types';
import ConnectionPanel from './ConnectionPanel.vue';
import ConnectionStatus from './ConnectionStatus.vue';
import ServerAddress from './ServerAddress.vue';
import SelectMenu from '../SelectMenu.vue';
import AppMenu from '../AppMenu.vue';
import UiGlyph from '../UiGlyph.vue';
import { privateServerText } from '@shared/serverPrivacy';
const props = defineProps<{
    server: ServerEntry;
    ping: ServerPingResult | null;
    /** 服务器真实头像（SLP favicon 的 data URL；空 = 回退首字母） */
    icon?: string;
    pending: boolean;
    busy: boolean;
    running?: boolean;
    binding: boolean;
    targets: InstalledVersion[];
    bound: string;
    missing: boolean;
    lastUsed: string;
    targetToken: (target: InstalledVersion) => string;
    targetLabel: (target: InstalledVersion) => string;
    addressRevealed: boolean;
}>();
const emit = defineEmits<{
    bind: [value: string];
    connect: [];
    refresh: [];
    edit: [];
    remove: [];
    relink: [];
    versions: [];
    copy: [];
    address: [];
}>();

/* 「更多」溢出菜单（AppMenu 收编，替代自造 details 浮层） */
const moreOpen = ref(false);
const moreAnchor = ref<HTMLElement | null>(null);
const moreItems = computed(() => [
    {
        id: 'remove',
        label: '删除服务器',
        danger: true,
        disabled: props.busy || props.binding,
        disabledReason: props.busy
            ? '正在启动游戏，请等待'
            : props.binding
              ? '正在保存实例关联'
              : undefined
    }
]);
/* 实例选择（SelectMenu 收编）：首项为「未关联」占位，与原 option 语义一致 */
const boundOptions = computed(() => [
    { value: '', label: '未关联实例 · 连接时选择' },
    ...props.targets.map((v) => ({
        value: props.targetToken(v),
        label: props.targetLabel(v) + (v.isolated ? '（隔离）' : '')
    }))
]);
</script>
<template>
    <ConnectionPanel title="连接服务器" class="server-detail">
        <template #action
            ><button class="btn btn-ghost btn-sm" :disabled="pending" @click="$emit('refresh')">
                {{ pending ? '检测中…' : '刷新' }}</button
            ><ConnectionStatus
                :tone="pending ? 'pending' : ping?.online ? 'success' : 'neutral'"
                :label="pending ? '检测中' : ping?.online ? '在线' : ping ? '未连通' : '未检测'"
        /></template>
        <div data-ui="ServerDetails:bcfe4bdd0062" class="server-detail-title">
            <img
                v-if="icon"
                data-ui="ServerDetails:d5eb9cf8090b"
                class="server-monogram large server-detail-icon"
                :src="icon"
                alt=""
                draggable="false"
                aria-hidden="true"
            />
            <span
                v-else
                data-ui="ServerDetails:d5eb9cf8090b"
                class="server-monogram large"
                aria-hidden="true"
                >{{
                    privateServerText(server.name, server, addressRevealed)
                        .slice(0, 1)
                        .toUpperCase()
                }}</span
            >
            <div>
                <h3 data-ui="ServerDetails:30996065a917">
                    {{ privateServerText(server.name, server, addressRevealed) }}
                </h3>
                <ServerAddress
                    :address="server.address"
                    :revealed="addressRevealed"
                    copyable
                    @toggle="$emit('address')"
                    @copy="$emit('copy')"
                />
            </div>
        </div>
        <div
            data-ui="ServerDetails:8c10b252815e"
            v-if="ping?.motd && !pending"
            class="server-description"
            aria-live="polite"
        >
            {{ privateServerText(ping.motd, server, addressRevealed) }}
        </div>
        <div data-ui="ServerDetails:9358a88a51a0" class="server-facts">
            <div>
                <span>在线玩家</span
                ><strong>{{
                    ping?.online && !pending
                        ? privateServerText(ping.players, server, addressRevealed)
                        : '—'
                }}</strong>
            </div>
            <div>
                <span>网络延迟</span
                ><strong>{{ ping?.online && !pending ? ping.latencyMs + ' ms' : '—' }}</strong>
            </div>
        </div>
        <p
            data-ui="ServerDetails:8faafdb2bb97"
            v-if="ping?.online && !pending"
            class="connection-muted"
        >
            服务器版本：{{ privateServerText(ping.version, server, addressRevealed) }}
        </p>
        <label data-ui="ServerDetails:59f1e2f8c71a" class="connection-field"
            >使用此实例连接<!-- 原生 select → SelectMenu 收编（data-ui 经 fallthrough 落在触发钮） --><SelectMenu
                data-ui="ServerDetails:0a9558147a76"
                :model-value="bound"
                :disabled="busy || binding"
                :options="boundOptions"
                @update:model-value="$emit('bind', $event)"
            /></label
        >
        <div data-ui="ServerDetails:49a9cb995a70" v-if="missing" class="connection-result">
            <ConnectionStatus tone="danger" label="关联实例缺失" />
            <p>原实例可能已移动或所在磁盘不可用。</p>
            <div data-ui="ServerDetails:5a115acf33b4" class="connection-actions">
                <button
                    data-ui="ServerDetails:cc40b80410ef"
                    class="btn btn-ghost"
                    @click="$emit('relink')"
                >
                    重新关联</button
                ><button
                    data-ui="ServerDetails:172ad57da113"
                    class="btn btn-ghost"
                    @click="$emit('versions')"
                >
                    前往版本页
                </button>
            </div>
        </div>
        <p
            data-ui="ServerDetails:d3a016071ead"
            v-else-if="server.candidateVersionIds?.length"
            class="connection-muted"
        >
            共享目录记录，请选择并确认具体实例。
        </p>
        <div data-ui="ServerDetails:1e93687b1aac" class="connection-muted">
            <p data-ui="ServerDetails:1399ead01b1a" v-if="server.minecraftVersion">
                Minecraft {{ server.minecraftVersion
                }}<span data-ui="ServerDetails:2f795b8f4707" v-if="server.loader">
                    · {{ server.loader }} {{ server.loaderVersion }}</span
                >
            </p>
            <p>{{ lastUsed }}</p>
        </div>
        <button
            data-ui="ServerDetails:1081b0ad3610"
            class="btn btn-gold server-connect"
            :disabled="busy || binding"
            @click="$emit('connect')"
        >
            {{
                running
                    ? '游戏已运行'
                    : busy
                      ? '正在启动…'
                      : server.versionId && !missing
                        ? '启动并连接'
                        : '选择实例并连接'
            }}<span data-ui="ServerDetails:82645c79195b" aria-hidden="true"
                ><UiGlyph name="play" :size="13"
            /></span>
        </button>
        <div data-ui="ServerDetails:afa5660e1f9f" class="connection-actions server-secondary">
            <button
                data-ui="ServerDetails:5d680c5d54c5"
                class="btn btn-ghost"
                :disabled="busy || binding"
                @click="$emit('edit')"
            >
                编辑服务器
            </button>
            <!-- 「更多」溢出菜单（AppMenu 收编：右端对齐，危险项入菜单） -->
            <button
                class="btn btn-ghost server-more-btn"
                :aria-expanded="moreOpen"
                aria-haspopup="menu"
                aria-label="更多服务器操作"
                @click="
                    moreOpen = !moreOpen;
                    moreAnchor = ($event.currentTarget as HTMLElement);
                "
            >
                <UiGlyph name="more" :size="14" />
            </button>
        </div>
        <AppMenu
            :open="moreOpen"
            :anchor="moreAnchor"
            align="end"
            aria-label="更多服务器操作"
            :items="moreItems"
            @select="$emit('remove')"
            @close="moreOpen = false"
        />
        <p
            data-ui="ServerDetails:571a6e757999"
            v-if="ping && !ping.online && !pending"
            class="connection-muted server-footnote"
        >
            状态检测失败不一定代表无法进入游戏，仍可尝试连接。
        </p>
    </ConnectionPanel>
</template>

<style scoped>
.server-address {
    background: none;
    border: 0;
    color: var(--text-dim);
    font: inherit;
    font-size: 13px;
    padding: 0;
    text-align: left;
    overflow-wrap: anywhere;
    cursor: copy;
}
.server-more-btn {
    padding: 0 10px;
}
.server-connect {
    min-height: 44px;
}
.server-secondary {
    justify-content: flex-end;
}
.server-detail :deep(.connection-panel-head) {
    padding: 12px 20px;
}
.server-detail-title {
    gap: 12px;
}
.server-detail-icon {
    object-fit: cover;
    image-rendering: pixelated;
}
</style>
