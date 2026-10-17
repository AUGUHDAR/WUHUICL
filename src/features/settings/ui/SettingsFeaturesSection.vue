<script setup lang="ts">
import { store } from '../../../store';
import { useSettingsPersistence } from './useSettingsPersistence';

defineProps<{ visible: boolean }>();
const { save } = useSettingsPersistence();
const featureToggles = [
    { key: 'mods', label: '模组（资源管理）' },
    { key: 'packs', label: '资源包' },
    { key: 'shaders', label: '光影包' },
    { key: 'recordings', label: '录像' },
    { key: 'bridge', label: 'MOD 面板' },
    { key: 'servers', label: '服务器' },
    { key: 'friends', label: '联机' },
    { key: 'keys', label: '默认配置' },
    { key: 'skins', label: '皮肤与披风' },
    { key: 'community', label: '社区资源' }
];

function toggleFeature(key: string, enabled: boolean) {
    const current = store.settings?.disabledFeatures ?? [];
    const next = enabled ? current.filter((value) => value !== key) : [...new Set([...current, key])];
    void save({ disabledFeatures: next });
}
</script>

<template>
    <details
        data-ui="SettingsView:12f88af0d6bb"
        v-show="visible"
        data-section="features"
        class="card group collapse"
        open
    >
        <summary data-ui="SettingsView:77831ba75241" class="collapse-head">
            <h3 data-ui="SettingsView:a6c68223d774" class="group-title">功能管理</h3>
            <span data-ui="SettingsView:f0f97b5ffa53" class="collapse-arrow" aria-hidden="true"></span>
        </summary>
        <div data-ui="SettingsView:d104611781f4" class="collapse-body feature-grid">
            <p data-ui="SettingsView:a37f6be09b43" class="muted group-hint">
                关闭的功能将从侧边栏隐藏入口。核心功能（首页/游戏/设置）不可关闭。
            </p>
            <template v-for="feature in featureToggles" :key="feature.key">
                <h4
                    data-ui="SettingsView:67ed5304d938"
                    v-if="feature.key === 'mods' || feature.key === 'friends'"
                    class="feature-section-title"
                >
                    {{ feature.key === 'mods' ? '资源管理' : '其他功能' }}
                </h4>
                <div data-ui="SettingsView:bd3c6919b18c" class="feature-row">
                    <span data-ui="SettingsView:fa3cacde91cf" class="feature-name">{{
                        feature.label
                    }}</span>
                    <span data-ui="SettingsView:ddd6149b4fc5" class="switch">
                        <input
                            data-ui="SettingsView:907bb2e68f36"
                            type="checkbox"
                            :checked="!store.settings!.disabledFeatures.includes(feature.key)"
                            @change="
                                toggleFeature(
                                    feature.key,
                                    ($event.target as HTMLInputElement).checked
                                )
                            "
                        />
                        <span data-ui="SettingsView:5ef29076637c" class="switch-ui"></span>
                    </span>
                </div>
            </template>
        </div>
    </details>
</template>

<style scoped>
.feature-grid {
    display: grid;
}
.feature-section-title {
    font-size: 12px;
    color: var(--accent-2);
    margin: 8px 0 2px;
}
.feature-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    padding: 10px 0;
    border-bottom: 1px solid var(--border);
}
.feature-row:last-child {
    border-bottom: 0;
}
.feature-name {
    font-size: 13px;
}
</style>
