<script setup lang="ts">
import { store } from '../../../store';
import { useSettingsPersistence } from './useSettingsPersistence';

defineProps<{ visible: boolean }>();
const { save } = useSettingsPersistence();
</script>

<template>
    <div
        v-show="visible"
        data-ui="SettingsView:35d4b384db5b"
        data-section="launch"
        class="settings-grid"
    >
        <div data-ui="SettingsView:67dc06128cde" class="card group group-inline">
            <div data-ui="SettingsView:f4183aab56c6">
                <h3 data-ui="SettingsView:984f729eb605" class="group-title">启动后关闭启动器</h3>
                <p data-ui="SettingsView:b44d69e716b0" class="muted group-hint">
                    游戏成功启动后自动退出 WUHUICL
                </p>
            </div>
            <label data-ui="SettingsView:9a7401ab1a9a" class="switch">
                <input
                    data-ui="SettingsView:f2f1ffcd2e84"
                    v-model="store.settings!.closeAfterLaunch"
                    type="checkbox"
                    @change="save({ closeAfterLaunch: store.settings!.closeAfterLaunch })"
                />
                <span data-ui="SettingsView:1500dc389a2c" class="switch-ui"></span>
            </label>
        </div>
        <div data-ui="SettingsView:39f185083aa5" class="card group group-inline">
            <div data-ui="SettingsView:83aa14cbc25b">
                <h3 data-ui="SettingsView:ebf81cec787e" class="group-title">
                    正版登录使用系统代理
                </h3>
                <p data-ui="SettingsView:661c21a8ae0c" class="muted group-hint">
                    微软登录失败而浏览器正常时可开启。默认直连；开启后仍保留 TLS 证书校验。
                </p>
            </div>
            <label data-ui="SettingsView:a729c0c443f6" class="switch">
                <input
                    data-ui="SettingsView:41ae714db129"
                    :checked="store.settings!.msUseProxy === true"
                    type="checkbox"
                    @change="
                        save({
                            msUseProxy: ($event.target as HTMLInputElement).checked
                        })
                    "
                />
                <span data-ui="SettingsView:e5b45bd3ff81" class="switch-ui"></span>
            </label>
        </div>
    </div>
</template>

<style scoped>
.settings-grid {
    display: grid;
    gap: 12px;
}
</style>
