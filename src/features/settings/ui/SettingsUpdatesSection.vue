<script setup lang="ts">
import UpdateSources from './UpdateSources.vue';
import { store } from '../../../store';
import { QQ_GROUP_NUMBER } from '@shared/branding';
import type { ReleaseInfo, UpdateStateInfo } from '@shared/types';

defineProps<{
    visible: boolean;
    appVersion: string;
    checkState: 'idle' | 'checking' | 'latest' | 'failed';
    pendingUpdate: { release: ReleaseInfo; file: string } | null;
    updateState: UpdateStateInfo | null;
    restoringBackup: boolean;
    systemInstaller: boolean;
    installAction: string;
    updateReadyMessage: (prefix?: string) => string;
}>();
const emit = defineEmits<{
    check: [];
    licenses: [];
    applyPending: [];
    rollback: [];
    restoreBackup: [];
    pickLocal: [];
    autoUpdate: [enabled: boolean];
}>();
</script>

<template>
    <details
        data-ui="SettingsView:7f23eec9c558"
        v-show="visible"
        class="card group collapse"
        open
        data-section="update"
    >
        <summary data-ui="SettingsView:557954d1d62f" class="collapse-head">
            <h3 data-ui="SettingsView:5b11a1127f08" class="group-title">关于与更新</h3>
            <span data-ui="SettingsView:f4e8d0d1c4b5" class="collapse-arrow" aria-hidden="true"></span>
        </summary>
        <div data-ui="SettingsView:37c9e71b5f27" class="collapse-body">
            <div data-ui="SettingsView:adafc6ff9288" class="upd-row">
                <span data-ui="SettingsView:3aeed3207223" class="upd-label">当前版本</span>
                <span data-ui="SettingsView:7f2b62d69ce5" class="upd-value">v{{ appVersion }}</span>
                <button
                    data-ui="SettingsView:e3f1fccf0c78"
                    class="btn btn-ghost btn-sm"
                    @click="emit('licenses')"
                >
                    第三方许可
                </button>
                <button
                    data-ui="SettingsView:2d10e8cc0926"
                    class="btn btn-ghost btn-sm"
                    :disabled="checkState === 'checking'"
                    @click="emit('check')"
                >
                    <span
                        data-ui="SettingsView:bb9654fb7775"
                        v-if="checkState === 'checking'"
                        class="spin"
                    ></span>
                    {{ checkState === 'checking' ? '检查中' : '检查更新' }}
                </button>
                <span data-ui="SettingsView:cd41281572d3" v-if="checkState === 'latest'" class="upd-latest"
                    >已是最新 ✓</span
                >
            </div>
            <div
                data-ui="SettingsView:203842d2f6d4"
                v-if="checkState === 'failed'"
                class="upd-row upd-failed-row"
            >
                <span data-ui="SettingsView:5ae61f0d3e62" class="upd-failed-text"
                    >你的网络可能无法连接 GitHub，检查更新失败</span
                >
                <button
                    data-ui="SettingsView:c30ef48b6984"
                    class="btn btn-ghost btn-sm"
                    @click="emit('check')"
                >
                    重试
                </button>
            </div>
            <div data-ui="SettingsView:a74dc3e10dc2" class="upd-row">
                <span data-ui="SettingsView:ff94c1ff533f" class="upd-label">自动安装更新</span>
                <label data-ui="SettingsView:ca6bc7bd4811" class="switch">
                    <input
                        data-ui="SettingsView:bc540ac861a7"
                        :checked="store.settings!.autoUpdate !== false"
                        type="checkbox"
                        @change="
                            emit(
                                'autoUpdate',
                                ($event.target as HTMLInputElement).checked
                            )
                        "
                    />
                    <span data-ui="SettingsView:9a339a81103f" class="switch-ui"></span>
                </label>
                <span data-ui="SettingsView:94871aad1da8" class="muted upd-auto-hint">{{
                    systemInstaller
                        ? '发现新版本静默下载，下次启动打开系统安装器；关闭则弹窗询问'
                        : '发现新版本静默下载，下次启动时应用；关闭则弹窗询问'
                }}</span>
            </div>
            <div
                data-ui="SettingsView:1c8ad9d5ad0b"
                v-if="pendingUpdate"
                class="upd-row upd-pending-row"
            >
                <span data-ui="SettingsView:0447f8159ad6" class="upd-pending-text">{{
                    updateReadyMessage(`v${pendingUpdate.release.version} 已就绪`)
                }}</span>
                <button
                    data-ui="SettingsView:5bd17b41383a"
                    class="btn btn-gold btn-sm"
                    @click="emit('applyPending')"
                >
                    {{ installAction }}
                </button>
            </div>
            <UpdateSources />
            <details data-ui="SettingsView:91c2bf912316" class="update-maintenance">
                <summary data-ui="SettingsView:aa8b3fd4177b">维护与恢复</summary>
                <div data-ui="SettingsView:bb7974b118ee" class="upd-row upd-actions-row">
                    <button
                        data-ui="SettingsView:3752063389f8"
                        class="btn btn-ghost btn-sm"
                        @click="emit('rollback')"
                    >
                        版本回退…
                    </button>
                    <button
                        data-ui="SettingsView:a671c86452da"
                        v-if="updateState"
                        class="btn btn-ghost btn-sm"
                        :disabled="restoringBackup"
                        @click="emit('restoreBackup')"
                    >
                        还原到更新前的版本（v{{ updateState.backupVersion }}）
                    </button>
                    <button
                        data-ui="SettingsView:c7a44c22390c"
                        class="btn btn-ghost btn-sm"
                        @click="emit('pickLocal')"
                    >
                        从本地文件安装更新…
                    </button>
                </div>
            </details>
            <p data-ui="SettingsView:108b83a3465d" class="muted group-hint">
                更新包同时发布在 GitHub 与 Gitee（国内镜像）；下载较慢时可在设置 →
                更新源里改用镜像，或到 WUHUICL 群（{{
                    store.settings!.qqGroupNumber?.trim() || QQ_GROUP_NUMBER
                }}）获取，两处文件与 GitHub 版本一致。
            </p>
        </div>
    </details>
</template>

<style scoped>
.upd-row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
}
.upd-label {
    min-width: 90px;
    font-size: 13px;
}
.upd-value,
.upd-latest {
    font-weight: 600;
}
.upd-latest,
.upd-ok {
    color: var(--success);
}
.upd-failed-text,
.upd-warn {
    color: var(--danger);
}
.upd-auto-hint {
    flex: 1;
    min-width: 220px;
    font-size: 11px;
    line-height: 1.6;
}
.upd-pending-row {
    padding: 10px;
    border: 1px solid var(--accent);
    border-radius: var(--radius-sm);
    background: var(--accent-soft);
}
.upd-pending-text {
    flex: 1;
    font-size: 12px;
}
.update-maintenance {
    margin-top: 6px;
}
.update-maintenance > summary {
    cursor: pointer;
    font-size: 13px;
}
.upd-actions-row {
    margin-top: 10px;
}
</style>
