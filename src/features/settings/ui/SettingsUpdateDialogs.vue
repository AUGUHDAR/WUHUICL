<script setup lang="ts">
import UpdateDialogShell from '../../updates/ui/UpdateDialogShell.vue';
import type { LocalUpdateCheck, ReleaseInfo } from '@shared/types';

defineProps<{
    appVersion: string;
    rollback: { open: boolean; loading: boolean; list: ReleaseInfo[]; selected: string };
    localUpdate: { check: LocalUpdateCheck; confirming: boolean } | null;
    releaseSummary: (body: string) => string;
    releaseDate: (value: string) => string;
}>();
const emit = defineEmits<{
    closeRollback: [];
    closeLocal: [];
    confirmRollback: [];
    confirmLocal: [];
}>();
</script>

<template>
    <UpdateDialogShell v-if="rollback.open" label="版本回退" @dismiss="emit('closeRollback')">
        <template #header>
            <div data-ui="SettingsView:4df775f4a3b9" class="upd-modal-head">
                <h3 data-ui="SettingsView:8225012f972d" class="upd-modal-title">版本回退</h3>
                <button
                    data-ui="SettingsView:606774134bae"
                    class="icon-btn"
                    title="关闭"
                    @click="emit('closeRollback')"
                >
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                    >
                        <path d="M18 6 6 18M6 6l12 12" />
                    </svg>
                </button>
            </div>
            <p data-ui="SettingsView:f683c36eb3d1" class="upd-modal-subtitle muted">
                当前 v{{ appVersion }} · 选择要恢复的历史版本
            </p>
        </template>
        <p data-ui="SettingsView:e60d0b9f406f" class="upd-risk">
            ⚠ 旧版本可能不兼容新配置格式。回退前将自动备份当前版本，可随时还原。
        </p>
        <div data-ui="SettingsView:057bbe26b76d" v-if="rollback.loading" class="empty">
            <span data-ui="SettingsView:4f0800807c40" class="spin"></span>
        </div>
        <div data-ui="SettingsView:5fc7aa4d0d0b" v-else class="upd-release-list">
            <label
                data-ui="SettingsView:75b890b63a58"
                v-for="release in rollback.list"
                :key="release.version"
                class="upd-release-item"
                :class="{ selected: rollback.selected === release.version }"
            >
                <input
                    data-ui="SettingsView:839c64939132"
                    v-model="rollback.selected"
                    type="radio"
                    name="rollback-version"
                    :value="release.version"
                />
                <span data-ui="SettingsView:147be114439d" class="upd-release-main">
                    <span data-ui="SettingsView:472e50c1be99" class="upd-release-ver"
                        >v{{ release.version }}</span
                    >
                    <span data-ui="SettingsView:f2396c2f4962" class="muted upd-release-date">{{
                        releaseDate(release.publishedAt)
                    }}</span>
                    <span
                        data-ui="SettingsView:b10e6198b84f"
                        v-if="releaseSummary(release.body)"
                        class="muted upd-release-summary"
                        >{{ releaseSummary(release.body) }}</span
                    >
                </span>
            </label>
            <div data-ui="SettingsView:b683a5ef98a2" v-if="!rollback.list.length" class="empty">
                <span data-ui="SettingsView:3a0956417c69">没有可回退的历史版本</span>
            </div>
        </div>
        <template #footer>
            <div data-ui="SettingsView:6900010059e5" class="upd-modal-actions">
                <button
                    data-ui="SettingsView:64d5b7c61143"
                    class="btn btn-ghost"
                    @click="emit('closeRollback')"
                >
                    取消
                </button>
                <button
                    data-ui="SettingsView:8a817c12df3a"
                    class="btn btn-gold"
                    :disabled="!rollback.selected"
                    @click="emit('confirmRollback')"
                >
                    回退到选中版本
                </button>
            </div>
        </template>
    </UpdateDialogShell>

    <UpdateDialogShell
        v-if="localUpdate?.confirming"
        label="安装本地更新包"
        @dismiss="emit('closeLocal')"
    >
        <template #header>
            <div data-ui="SettingsView:e3b6bce81002" class="upd-modal-head">
                <h3 data-ui="SettingsView:5b400fb54b75" class="upd-modal-title">
                    安装本地更新包
                </h3>
                <button
                    data-ui="SettingsView:7162c61cf755"
                    class="icon-btn"
                    title="关闭"
                    @click="emit('closeLocal')"
                >
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                    >
                        <path d="M18 6 6 18M6 6l12 12" />
                    </svg>
                </button>
            </div>
        </template>
        <p data-ui="SettingsView:d8da86ab7cbc" class="upd-local-file">
            {{ localUpdate.check.fileName }} ·
            {{ (localUpdate.check.fileSize / 1048576).toFixed(1) }} MB
        </p>
        <div data-ui="SettingsView:6bd0a3b9b887" class="upd-local-check">
            <span data-ui="SettingsView:4d74e5534f41">版本校验</span>
            <span
                data-ui="SettingsView:152ca71f54e0"
                v-if="localUpdate.check.versionOk"
                class="upd-ok"
                >v{{ localUpdate.check.version }} ≥ 当前 v{{ appVersion }} ✓</span
            >
            <span data-ui="SettingsView:e950885a700c" v-else class="upd-warn"
                >⚠
                {{
                    localUpdate.check.version
                        ? `v${localUpdate.check.version} 低于当前 v${appVersion}`
                        : '无法从文件名识别版本号'
                }}，继续需自担风险</span
            >
        </div>
        <div data-ui="SettingsView:ee13e08992b5" class="upd-local-check">
            <span data-ui="SettingsView:b5e490092529">完整性校验</span>
            <span
                data-ui="SettingsView:9df9b415ad42"
                v-if="localUpdate.check.sha256 === 'match'"
                class="upd-ok"
                >SHA256 与 GitHub Release 一致 ✓</span
            >
            <span
                data-ui="SettingsView:5b6de8db67a7"
                v-else-if="localUpdate.check.sha256 === 'mismatch'"
                class="upd-warn"
                >⚠ SHA256 不一致！文件可能被篡改（{{ localUpdate.check.detail }}）</span
            >
            <span data-ui="SettingsView:f5a9e52452b4" v-else class="upd-warn"
                >⚠ 无法联网校验，请确认文件来自官方渠道，风险自担</span
            >
        </div>
        <template #footer>
            <div data-ui="SettingsView:49c0a8213107" class="upd-modal-actions">
                <button
                    data-ui="SettingsView:3ccc5837a1e1"
                    class="btn btn-ghost"
                    @click="emit('closeLocal')"
                >
                    取消
                </button>
                <button
                    data-ui="SettingsView:1b671fb6ec3e"
                    class="btn"
                    :class="
                        localUpdate.check.versionOk && localUpdate.check.sha256 === 'match'
                            ? 'btn-gold'
                            : 'btn-danger'
                    "
                    @click="emit('confirmLocal')"
                >
                    确认安装
                </button>
            </div>
        </template>
    </UpdateDialogShell>
</template>

<style scoped>
.upd-modal-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
}
.upd-modal-title {
    margin: 0;
    font-size: 20px;
}
.upd-modal-subtitle {
    margin: 6px 0 0;
    font-size: 12px;
}
.upd-risk {
    color: var(--danger);
    font-size: 13px;
    line-height: 1.7;
}
.upd-release-list {
    display: grid;
    gap: 8px;
}
.upd-release-item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
}
.upd-release-item.selected {
    border-color: var(--accent);
    background: var(--accent-soft);
}
.upd-release-main {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 4px;
}
.upd-release-ver {
    font-weight: 600;
}
.upd-release-date,
.upd-release-summary {
    font-size: 12px;
}
.upd-modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
}
.upd-local-file {
    overflow-wrap: anywhere;
}
.upd-local-check {
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 10px 0;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
}
.upd-ok {
    color: var(--success);
}
.upd-warn {
    color: var(--danger);
}
</style>
