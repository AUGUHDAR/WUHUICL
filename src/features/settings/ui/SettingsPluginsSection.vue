<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { errText, installPlugin, listPlugins, openPluginsDir, removePlugin, setPluginEnabled } from '../../../api';
import { toast } from '../../../store';
import type { PluginInfo } from '@shared/types';

defineProps<{ visible: boolean }>();
const plugins = ref<PluginInfo[]>([]);
const pluginBusy = ref(false);
const pluginDirty = ref(false);
const pluginConfirmRemove = ref('');

onMounted(async () => {
    try {
        plugins.value = await listPlugins();
    } catch {
        /* A plugin failure must not block the settings page. */
    }
});

async function install() {
    if (pluginBusy.value) return;
    pluginBusy.value = true;
    try {
        const before = plugins.value.length;
        plugins.value = await installPlugin();
        if (plugins.value.length > before) {
            pluginDirty.value = true;
            toast('插件已安装，重载启动器后生效', 'success');
        }
    } catch (error) {
        toast('安装失败：' + errText(error), 'error');
    } finally {
        pluginBusy.value = false;
    }
}

async function toggle(plugin: PluginInfo, enabled: boolean) {
    try {
        plugins.value = await setPluginEnabled(plugin.id, enabled);
        pluginDirty.value = true;
    } catch (error) {
        toast('操作失败：' + errText(error), 'error');
    }
}

async function remove(plugin: PluginInfo) {
    if (pluginConfirmRemove.value !== plugin.id) {
        pluginConfirmRemove.value = plugin.id;
        setTimeout(() => {
            if (pluginConfirmRemove.value === plugin.id) pluginConfirmRemove.value = '';
        }, 3000);
        return;
    }
    pluginConfirmRemove.value = '';
    try {
        plugins.value = await removePlugin(plugin.id);
        pluginDirty.value = true;
        toast(`插件已移入回收站：${plugin.name}`, 'success');
    } catch (error) {
        toast('删除失败：' + errText(error), 'error');
    }
}
</script>

<template>
    <details
        data-ui="SettingsView:034daafb11ff"
        v-show="visible"
        data-section="plugins"
        class="card group collapse"
        open
    >
        <summary data-ui="SettingsView:c48b3c27dd89" class="collapse-head">
            <h3 data-ui="SettingsView:539396c2c42e" class="group-title">插件</h3>
            <span data-ui="SettingsView:23f8f0d45fa8" class="collapse-arrow" aria-hidden="true"></span>
        </summary>
        <div data-ui="SettingsView:7d99322bac74" class="collapse-body">
            <p data-ui="SettingsView:89be6deab034" class="muted group-hint">
                JS 插件可更改界面、新增功能（启动器版
                Mod）。插件拥有界面完全控制权，请只安装可信来源。
            </p>
            <div data-ui="SettingsView:3610ad90128d" v-if="plugins.length" class="plugin-list">
                <div
                    data-ui="SettingsView:f0df70c46e6e"
                    v-for="plugin in plugins"
                    :key="plugin.id"
                    class="plugin-row"
                >
                    <div data-ui="SettingsView:f0ef3e1815ac" class="plugin-info">
                        <span data-ui="SettingsView:2654f6c0658e" class="plugin-name">
                            {{ plugin.name }}
                            <span
                                data-ui="SettingsView:ed121d4fcf9e"
                                v-if="plugin.version"
                                class="muted"
                                >v{{ plugin.version }}</span
                            >
                        </span>
                        <span data-ui="SettingsView:2651a76d5a4b" class="muted plugin-meta">{{
                            [plugin.author, plugin.description].filter(Boolean).join(' · ') ||
                            plugin.id
                        }}</span>
                    </div>
                    <button
                        data-ui="SettingsView:4b6726b9ed78"
                        class="btn btn-sm"
                        :class="pluginConfirmRemove === plugin.id ? 'btn-danger' : 'btn-ghost'"
                        @click="remove(plugin)"
                    >
                        {{ pluginConfirmRemove === plugin.id ? '移入回收站' : '删除' }}
                    </button>
                    <label
                        data-ui="SettingsView:1df38fea5b7b"
                        class="switch"
                        :title="plugin.enabled ? '停用插件' : '启用插件'"
                    >
                        <input
                            data-ui="SettingsView:e7c877c0882e"
                            type="checkbox"
                            :checked="plugin.enabled"
                            @change="
                                toggle(
                                    plugin,
                                    ($event.target as HTMLInputElement).checked
                                )
                            "
                        />
                        <span data-ui="SettingsView:28edec50edc5" class="switch-ui"></span>
                    </label>
                </div>
            </div>
            <p data-ui="SettingsView:fe8c8a6583e4" v-else class="muted group-hint">
                还没有安装插件
            </p>
            <div data-ui="SettingsView:70c23a4a8b21" class="plugin-actions">
                <button
                    data-ui="SettingsView:2efffb3d4610"
                    class="btn btn-ghost btn-sm"
                    :disabled="pluginBusy"
                    @click="install"
                >
                    <span data-ui="SettingsView:f506d1f658b6" v-if="pluginBusy" class="spin"></span>
                    安装插件（.js）
                </button>
                <button
                    data-ui="SettingsView:5032f71dc65d"
                    class="btn btn-ghost btn-sm"
                    @click="openPluginsDir"
                >
                    打开插件目录
                </button>
                <button
                    data-ui="SettingsView:ede794e781ff"
                    v-if="pluginDirty"
                    class="btn btn-gold btn-sm"
                    @click="
                        ($event.target as HTMLButtonElement).blur();
                        location.reload();
                    "
                >
                    重载启动器生效
                </button>
            </div>
        </div>
    </details>
</template>

<style scoped>
.plugin-list {
    display: grid;
    gap: 4px;
}
.plugin-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 0;
    border-bottom: 1px solid var(--border);
}
.plugin-info {
    display: flex;
    flex: 1;
    min-width: 0;
    flex-direction: column;
    gap: 3px;
}
.plugin-name {
    font-size: 13px;
    font-weight: 600;
}
.plugin-meta {
    overflow: hidden;
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.plugin-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 10px;
}
</style>
