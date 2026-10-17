<script setup lang="ts">
import { computed, ref } from 'vue';
import SelectMenu from '../../../components/SelectMenu.vue';
import { errText, getSettings, selectDir, setDownloadFolder } from '../../../api';
import { refreshInstalled, store, toast } from '../../../store';
import { useSettingsPersistence } from './useSettingsPersistence';

defineProps<{ visible: boolean }>();
const { save } = useSettingsPersistence();
const defaultDownloadFolder = computed(
    () =>
        store.settings?.folders.find((folder) => folder.isDefault)?.path ||
        store.settings?.activeFolder ||
        store.settings?.gameDir ||
        ''
);
const downloadFolderBusy = ref(false);
const downloadFolderError = ref('');

async function applyDownloadFolder(folder: string) {
    if (!folder || downloadFolderBusy.value) return;
    downloadFolderBusy.value = true;
    downloadFolderError.value = '';
    let committed = false;
    try {
        const folders = await setDownloadFolder(folder);
        committed = true;
        const selected = folders.find((item) => item.isDefault)!.path;
        if (store.settings)
            store.settings = {
                ...store.settings,
                folders,
                activeFolder: selected,
                gameDir: selected
            };
        store.settings = await getSettings();
        store.resourceVersionId = '';
        await refreshInstalled();
        toast(
            '默认下载位置已更改；已有游戏目录和存档保留，正在下载的任务继续使用原位置',
            'success'
        );
    } catch (error) {
        downloadFolderError.value = committed
            ? '下载位置已保存，但列表暂未刷新。请重新打开页面重试：' + errText(error)
            : '更改失败：' + errText(error);
    } finally {
        downloadFolderBusy.value = false;
    }
}
async function chooseNewDownloadFolder() {
    if (downloadFolderBusy.value) return;
    downloadFolderBusy.value = true;
    downloadFolderError.value = '';
    try {
        const selected = await selectDir();
        downloadFolderBusy.value = false;
        if (selected) await applyDownloadFolder(selected);
    } catch (error) {
        downloadFolderError.value = '选择文件夹失败：' + errText(error);
    } finally {
        downloadFolderBusy.value = false;
    }
}
</script>

<template>
    <div
        v-show="visible"
        data-section="installation"
        data-ui="download-location:settings"
        class="card group directory-setting"
    >
        <h3 data-ui="SettingsView:c5658552d90d" class="group-title">默认下载位置</h3>
        <p data-ui="SettingsView:eb7e5618a756" class="muted group-hint">
            新游戏版本、整合包和共享资源（依赖库、游戏素材、自动下载的 Java）保存在此处。
        </p>
        <div data-ui="SettingsView:d32e87dadcdf" class="dir-row download-location-row">
            <SelectMenu
                class="download-location-select"
                :model-value="defaultDownloadFolder"
                :options="
                    store.settings!.folders.map((folder) => ({
                        value: folder.path,
                        label: folder.name + (folder.isDefault ? '（默认）' : '')
                    }))
                "
                :disabled="downloadFolderBusy"
                @change="applyDownloadFolder"
            />
            <button
                class="btn btn-gold dir-btn"
                data-ui="download-location:change"
                :disabled="downloadFolderBusy"
                @click="chooseNewDownloadFolder"
            >
                {{ downloadFolderBusy ? '更改中…' : '更改…' }}
            </button>
            <button
                class="btn btn-ghost dir-btn"
                data-ui="download-location:manage"
                :disabled="downloadFolderBusy"
                @click="store.currentView = 'game'"
            >
                游戏文件夹管理
            </button>
        </div>
        <input
            data-ui="download-location:path"
            class="input mono download-location-path"
            :value="defaultDownloadFolder"
            readonly
            aria-label="默认下载位置"
            :title="defaultDownloadFolder"
        />
        <p data-ui="SettingsView:c7fc97abe7a2" class="muted group-hint">
            更改后只影响新任务；已有游戏、存档和正在下载的任务保留原位置。导入时可单独选择目标文件夹。压缩包下载与解压可能暂时使用系统临时目录。
        </p>
        <p
            v-if="downloadFolderError"
            class="error"
            role="alert"
            data-ui="download-location:error"
        >
            {{ downloadFolderError }}
        </p>
    </div>
    <div data-ui="SettingsView:9df9d3d48082" v-show="visible" class="settings-grid">
        <details
            data-ui="SettingsView:d0490715189c"
            class="card group collapse setting-target"
            data-section="downloads"
            tabindex="-1"
            open
        >
            <summary data-ui="SettingsView:0aa4b44c28d2" class="collapse-head">
                <h3 data-ui="SettingsView:3c521a2d07a0" class="group-title">下载</h3>
                <span
                    data-ui="SettingsView:53c4924a8276"
                    class="collapse-arrow"
                    aria-hidden="true"
                ></span>
            </summary>
            <div data-ui="SettingsView:7d7409e56805" class="collapse-body">
                <div
                    data-ui="SettingsView:95f82195a3e7"
                    data-section="mirror"
                    class="download-mirror-group"
                >
                    <div data-ui="SettingsView:df1b5ac2d829" class="download-mirror-options">
                        <h3 data-ui="SettingsView:377562d71363" class="group-title">下载镜像</h3>
                        <div data-ui="SettingsView:7039ff11fe3c" class="mirror-options">
                            <label
                                data-ui="SettingsView:3dd4d470872e"
                                class="mirror-option"
                                :class="{ active: store.settings!.mirror === 'official' }"
                            >
                                <input
                                    data-ui="SettingsView:2f522f16f088"
                                    v-model="store.settings!.mirror"
                                    type="radio"
                                    value="official"
                                    @change="save({ mirror: 'official' })"
                                />
                                <span data-ui="SettingsView:7f4f630df278">官方源</span>
                            </label>
                            <label
                                data-ui="SettingsView:4948cbbe2972"
                                class="mirror-option"
                                :class="{ active: store.settings!.mirror === 'bmclapi' }"
                            >
                                <input
                                    data-ui="SettingsView:6cf3093424b7"
                                    v-model="store.settings!.mirror"
                                    type="radio"
                                    value="bmclapi"
                                    @change="save({ mirror: 'bmclapi' })"
                                />
                                <span data-ui="SettingsView:1063a9cd6367"
                                    >BMCLAPI 镜像（国内更快）</span
                                >
                            </label>
                        </div>
                    </div>
                </div>
                <label data-ui="SettingsView:9ebdff18892c" class="download-setting"
                    >最大线程数
                    <input
                        data-ui="SettingsView:0975603bb2e6"
                        class="input"
                        type="number"
                        min="1"
                        max="64"
                        :value="store.settings!.downloadThreads"
                        @change="
                            save({
                                downloadThreads: Number(
                                    ($event.target as HTMLInputElement).value
                                )
                            })
                        "
                    />
                </label>
                <label data-ui="SettingsView:d4cc8ddc9838" class="download-setting"
                    >速度限制（KiB/s）
                    <input
                        data-ui="SettingsView:f6bb60f3451f"
                        class="input"
                        type="number"
                        min="0"
                        max="1048576"
                        :value="store.settings!.downloadSpeedKBps"
                        @change="
                            save({
                                downloadSpeedKBps: Number(
                                    ($event.target as HTMLInputElement).value
                                )
                            })
                        "
                    />
                </label>
                <p data-ui="SettingsView:90ea63c0b339" class="muted group-hint">
                    0
                    表示不限速。限制对全部下载任务合计生效；减少线程数后，已有连接完成时释放名额。
                </p>
                <details data-ui="SettingsView:d03dfc867050" class="advanced-setting">
                    <summary data-ui="SettingsView:98629fe42230">
                        高级 · CurseForge API Key
                    </summary>
                    <label
                        data-ui="SettingsView:5d8857a9ab59"
                        class="download-setting download-setting-col"
                    >
                        <span data-ui="SettingsView:4e5c4512e2a8"
                            >CurseForge API Key<small
                                data-ui="SettingsView:a100ae255935"
                                class="muted"
                                >（选填，官方 api.curseforge.com 通道）</small
                            ></span
                        >
                        <input
                            data-ui="SettingsView:bb44d7c926e9"
                            class="input mono"
                            type="password"
                            :value="store.settings!.curseforgeApiKey ?? ''"
                            placeholder="留空走镜像通道"
                            @change="
                                save({
                                    curseforgeApiKey: (
                                        $event.target as HTMLInputElement
                                    ).value.trim()
                                })
                            "
                        />
                    </label>
                    <p data-ui="SettingsView:2afaeb170207" class="muted group-hint">
                        CurseForge 官方 API 需要免费注册申请：<a
                            data-ui="SettingsView:b8eb49f2eba5"
                            class="upd-link"
                            href="https://console.curseforge.com/"
                            target="_blank"
                            rel="noreferrer"
                            >console.curseforge.com</a
                        >（注册 → 创建应用 → 复制 API Key 粘贴到上方）。填入后社区资源的
                        CurseForge 搜索与下载走官方通道，不受镜像波动影响。
                    </p>
                </details>
            </div>
        </details>
    </div>
</template>

<style scoped>
.download-setting {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-size: 13px;
}
.download-setting .input {
    width: 130px;
}
.download-setting-col {
    align-items: stretch;
    flex-direction: column;
}
.download-setting-col .input {
    width: 100%;
}
.download-setting-col small {
    font-size: 11px;
}
.dir-row {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
}
.download-location-select {
    flex: 1;
    min-width: 220px;
}
.download-location-path {
    width: 100%;
}
.mirror-options {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
}
.mirror-option {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-size: 13px;
}
.mirror-option.active {
    border-color: var(--accent);
    background: var(--accent-soft);
}
.advanced-setting {
    margin-top: 4px;
}
</style>
