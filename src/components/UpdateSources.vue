<script setup lang="ts">
import { computed, nextTick, ref } from 'vue';
import type { Settings } from '@shared/types';
import { store, toast } from '../store';
import { updateSettings } from '../settingsUpdates';
import { errText } from '../api';
import { restoreLostControlFocus } from '../controlFocus';

/** 旧档位迁移：direct=仅 GitHub、mirror=镜像优先（已并入 Gitee）；未知值回落到自动择优 */
function normalizeSource(value: unknown): 'auto' | 'github' | 'gitee' {
    if (value === 'github' || value === 'direct') return 'github';
    if (value === 'gitee' || value === 'mirror') return 'gitee';
    return 'auto';
}
const source = computed(() => normalizeSource(store.settings?.updateSource));
const busy = ref(false),
    error = ref('');
const help = computed(() =>
    source.value === 'gitee'
        ? '只通过 Gitee（国内直连）下载更新，不经过 GitHub。'
        : source.value === 'github'
          ? '只通过 GitHub 直连下载更新，不经过 Gitee。'
          : 'GitHub 与 Gitee 同时开下竞速，先到且校验通过的一方胜出。'
);
async function change(value: string) {
    if (busy.value) return;
    const previousFocus =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
    let failed = false;
    busy.value = true;
    error.value = '';
    try {
        await updateSettings({ updateSource: value as Settings['updateSource'] });
    } catch (failure) {
        failed = true;
        error.value = errText(failure);
        toast('保存更新来源失败：' + error.value, 'error');
    } finally {
        busy.value = false;
        if (failed) {
            await nextTick();
            restoreLostControlFocus(previousFocus, '.update-sources');
        }
    }
}
</script>

<template>
    <section data-ui="" class="update-sources" aria-label="更新下载来源">
        <div data-ui="SettingsView:ef2f8a060d8a" class="source-row">
            <label for="update-source">更新下载源</label
            ><select
                id="update-source"
                data-ui="SettingsView:c67695800552"
                class="select"
                :value="source"
                :disabled="busy"
                @change="change(($event.target as HTMLSelectElement).value)"
            >
                <option value="auto">自动择优（GitHub / Gitee）</option>
                <option value="gitee">仅 Gitee（国内直连）</option>
                <option value="github">仅 GitHub 直连</option>
            </select>
        </div>
        <p class="muted source-help" role="status">{{ help }}</p>
        <p v-if="error" class="source-error" role="alert">{{ error }}</p>
    </section>
</template>

<style scoped>
.update-sources {
    min-width: 0;
}
.source-row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    margin-top: 10px;
}
.source-row > label {
    font-size: 13px;
    min-width: 104px;
}
.source-row .select {
    width: 250px;
    max-width: 100%;
}
.source-help {
    font-size: 12px;
    line-height: 1.6;
    margin: 7px 0;
}
.source-error {
    color: var(--danger);
    font-size: 12px;
    line-height: 1.6;
}
</style>
