<script setup lang="ts">
import { ref } from 'vue';
import { store } from '../../../store';
import { useSettingsPersistence } from './useSettingsPersistence';

defineProps<{ visible: boolean }>();
const { save } = useSettingsPersistence();
const resolutionError = ref('');
const windowSizeBusy = ref(false);
const canRememberWindow = window.kamucl.platform === 'win32';

function saveResolution() {
    const settings = store.settings;
    if (!settings) return;
    const width = Number(settings.resolution.width);
    const height = Number(settings.resolution.height);
    if (!Number.isInteger(width) || width < 854 || width > 7680) {
        resolutionError.value = '窗口宽度必须是 854–7680 之间的整数';
        return;
    }
    if (!Number.isInteger(height) || height < 480 || height > 4320) {
        resolutionError.value = '窗口高度必须是 480–4320 之间的整数';
        return;
    }
    resolutionError.value = '';
    settings.resolution.fullscreen = settings.resolution.mode === 'fullscreen';
    void save({ resolution: { ...settings.resolution } });
}

async function toggleWindowSize(event: Event) {
    const input = event.target as HTMLInputElement;
    if (!store.settings || windowSizeBusy.value) return;
    windowSizeBusy.value = true;
    await save(
        { rememberGameWindowSize: input.checked },
        '保存窗口大小设置失败：'
    );
    windowSizeBusy.value = false;
    input.checked = store.settings?.rememberGameWindowSize === true;
}
</script>

<template>
    <div v-show="visible" data-ui="SettingsView:a1da38f817b5" class="settings-grid">
        <div data-ui="SettingsView:5265f547a736" class="card group" data-section="resolution">
            <h3 data-ui="SettingsView:47a461e3d7d0" class="group-title">游戏窗口分辨率</h3>
            <div data-ui="SettingsView:7e472ba8461b" class="resolution-row">
                <div data-ui="SettingsView:5cdd1adaad21" class="res-field">
                    <span data-ui="SettingsView:5ff70fc575bb" class="muted res-label">宽</span>
                    <input
                        data-ui="SettingsView:a5f1bf93348f"
                        v-model.number="store.settings!.resolution.width"
                        type="number"
                        class="input"
                        min="854"
                        max="7680"
                        :disabled="store.settings!.resolution.mode !== 'windowed'"
                        @change="saveResolution"
                    />
                </div>
                <span data-ui="SettingsView:bc7a0e5ae260" class="muted res-x">×</span>
                <div data-ui="SettingsView:a3e6df15fa0b" class="res-field">
                    <span data-ui="SettingsView:aa77b3a60182" class="muted res-label">高</span>
                    <input
                        data-ui="SettingsView:c9736d13ba36"
                        v-model.number="store.settings!.resolution.height"
                        type="number"
                        class="input"
                        min="480"
                        max="4320"
                        :disabled="store.settings!.resolution.mode !== 'windowed'"
                        @change="saveResolution"
                    />
                </div>
            </div>
            <div data-ui="SettingsView:f3d214648a71" class="resolution-mode">
                <span data-ui="SettingsView:90c57ea1d879" class="muted res-label">模式</span>
                <select
                    data-ui="SettingsView:6e8bb36bb0de"
                    v-model="store.settings!.resolution.mode"
                    class="select"
                    @change="saveResolution"
                >
                    <option value="windowed">窗口化</option>
                    <option value="maximized">最大化</option>
                    <option value="fullscreen">全屏</option>
                </select>
            </div>
            <p data-ui="SettingsView:2f107fd117d3" v-if="resolutionError" class="group-error">
                {{ resolutionError }}
            </p>
            <p data-ui="SettingsView:80e3d9022691" v-else class="muted group-hint">
                窗口化使用以上宽高；最大化使用启动时所在显示器的工作区；全屏不会修改显示器分辨率。
            </p>
            <div data-ui="SettingsView:fdad433cd601" class="remember-window-row">
                <div data-ui="SettingsView:e6e34f6fab36">
                    <strong data-ui="SettingsView:73276765bbcc"
                        >退出游戏自动保存窗口化大小</strong
                    >
                    <p data-ui="SettingsView:e62ce95cef85" class="muted group-hint">
                        启用后从下次启动开始记录正常窗口化尺寸；异常退出不覆盖设置，实例专属尺寸单独保存。
                    </p>
                </div>
                <label data-ui="SettingsView:aee9fe342146" class="switch">
                    <input
                        data-ui="SettingsView:1c13c6f8157b"
                        type="checkbox"
                        aria-label="退出游戏自动保存窗口化大小"
                        :checked="store.settings!.rememberGameWindowSize === true"
                        :disabled="windowSizeBusy || !canRememberWindow"
                        @change="toggleWindowSize"
                    />
                    <span data-ui="SettingsView:4833d59c75a6" class="switch-ui"></span>
                </label>
            </div>
            <p
                data-ui="SettingsView:d70854df0dee"
                v-if="!canRememberWindow"
                class="muted group-hint"
            >
                当前仅支持 Windows 游戏窗口。
            </p>
        </div>
    </div>
</template>

<style scoped>
.settings-grid {
    display: grid;
    gap: 12px;
}
.resolution-row {
    display: flex;
    align-items: flex-end;
    gap: 8px;
}
.res-field {
    display: grid;
    flex: 1;
    gap: 6px;
}
.res-label {
    font-size: 12px;
}
.res-x {
    padding-bottom: 9px;
}
.resolution-mode {
    display: flex;
    align-items: center;
    gap: 12px;
}
.resolution-mode .select {
    flex: 1;
}
.remember-window-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
}
.remember-window-row > div {
    flex: 1;
}
@media (max-width: 700px) {
    .remember-window-row {
        align-items: flex-start;
        flex-direction: column;
    }
}
</style>
