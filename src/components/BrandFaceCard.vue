<script setup lang="ts">
/**
 * 启动形象：开屏头像与左上角图标的来源配置。
 * 每个槽位三档来源：default=内置图 / account=当前登录账号皮肤（换号自动跟随）/
 * custom=自定义皮肤。上传的是 64×64 皮肤文件，前端渲染成方块头 PNG 后交后端
 * 落盘到受管 faces/ 目录（开屏窗口由主进程直接读取该文件）。
 */
import { reactive, ref, watch, onUnmounted } from 'vue';
import { store } from '../store';
import { updateSettings } from '../settingsUpdates';
import { errText, getSkinAvatar, resetFaceImage, saveFaceImage } from '../api';
import { managedImageUrl } from '../managedAssets';
import { renderSkinHead } from '../skin-render';
import { skinRevision } from '../skinRevision';
import defaultFace from '../assets/splash-face.png';

type Slot = 'splash' | 'logo';
type FaceSource = 'default' | 'account' | 'custom';

const slots: { key: Slot; title: string; desc: string }[] = [
    { key: 'splash', title: '开屏头像', desc: '启动器启动时全屏开屏页上的人物头像' },
    { key: 'logo', title: '左上角图标', desc: '侧边栏顶部的应用头像；点击仍可打开 AUGUHDAR 互动彩蛋' }
];

const sourceOf = (slot: Slot): FaceSource =>
    (store.settings?.[slot === 'splash' ? 'splashFaceSource' : 'logoFaceSource'] as FaceSource) ?? 'default';
const pathOf = (slot: Slot): string =>
    (store.settings?.[slot === 'splash' ? 'splashFacePath' : 'logoFacePath'] as string) ?? '';

const busy: Record<Slot, boolean> = reactive({ splash: false, logo: false });
const error: Record<Slot, string> = reactive({ splash: '', logo: '' });
const preview: Record<Slot, string> = reactive({ splash: '', logo: '' });

function fieldNames(slot: Slot) {
    return slot === 'splash'
        ? { source: 'splashFaceSource', path: 'splashFacePath' }
        : { source: 'logoFaceSource', path: 'logoFacePath' };
}

async function patchSource(slot: Slot, source: FaceSource, path?: string) {
    const names = fieldNames(slot);
    busy[slot] = true;
    error[slot] = '';
    try {
        await updateSettings({
            [names.source]: source,
            [names.path]: path ?? (source === 'default' ? '' : pathOf(slot))
        });
        return true;
    } catch (e) {
        error[slot] = errText(e);
        return false;
    } finally {
        busy[slot] = false;
    }
}

/** 切换来源；切到 custom 且还没有自定义图时直接引导上传（上传成功才落 custom 档） */
async function onSourceChange(slot: Slot, next: FaceSource) {
    if (next === 'custom' && !pathOf(slot)) {
        pickUpload(slot);
        return;
    }
    if (next === 'default' && pathOf(slot)) void resetFaceImage(slot);
    if (await patchSource(slot, next)) previewDirty(slot);
}

function onReset(slot: Slot) {
    void resetFaceImage(slot);
    void patchSource(slot, 'default');
}

// ---------------- 上传自定义皮肤 ----------------
const fileInput = ref<HTMLInputElement>();
let uploadSlot: Slot = 'splash';
function pickUpload(slot: Slot) {
    uploadSlot = slot;
    fileInput.value?.click();
}
function loadImage(src: string): Promise<HTMLImageElement> {
    return new Promise((resolve, reject) => {
        const img = new Image();
        img.onload = () => resolve(img);
        img.onerror = () => reject(new Error('皮肤图片无法加载'));
        img.src = src;
    });
}
async function onFile(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    const slot = uploadSlot;
    busy[slot] = true;
    error[slot] = '';
    try {
        const dataUrl = await new Promise<string>((resolve, reject) => {
            const reader = new FileReader();
            reader.onload = () => resolve(String(reader.result));
            reader.onerror = () => reject(new Error('皮肤文件读取失败'));
            reader.readAsDataURL(file);
        });
        const img = await loadImage(dataUrl);
        if (img.width !== 64 || (img.height !== 64 && img.height !== 32))
            throw new Error('皮肤必须是 64×64（或旧版 64×32）的 PNG 图片');
        const head = await renderSkinHead(img, 256);
        const path = await saveFaceImage(head, slot);
        const names = fieldNames(slot);
        await updateSettings({ [names.source]: 'custom', [names.path]: path });
        preview[slot] = managedImageUrl(path);
    } catch (e) {
        error[slot] = errText(e);
    } finally {
        busy[slot] = false;
    }
}

// ---------------- 预览 ----------------
let generation = 0;
onUnmounted(() => {
    generation++;
});
async function renderPreview(slot: Slot) {
    const request = ++generation;
    const source = sourceOf(slot);
    if (source === 'custom') {
        preview[slot] = pathOf(slot) ? managedImageUrl(pathOf(slot)) : defaultFace;
        return;
    }
    if (source === 'account') {
        const account = store.selectedAccount;
        if (!account) {
            preview[slot] = defaultFace;
            return;
        }
        const data = await getSkinAvatar(account.id).catch(() => null);
        if (request !== generation) return;
        preview[slot] = data ? await renderSkinHead(data, 128) : defaultFace;
        return;
    }
    preview[slot] = defaultFace;
}
function previewDirty(slot: Slot) {
    void renderPreview(slot);
}
watch(
    [
        () => sourceOf('splash'),
        () => pathOf('splash'),
        () => sourceOf('logo'),
        () => pathOf('logo'),
        () => store.selectedAccount?.id,
        skinRevision
    ],
    () => {
        void renderPreview('splash');
        void renderPreview('logo');
    },
    { immediate: true }
);
</script>

<template>
    <details
        data-ui="BrandFaceCard:root"
        class="card group collapse setting-target"
        data-section="brand-face"
        open
    >
        <summary data-ui="BrandFaceCard:head" class="collapse-head">
            <h3 data-ui="BrandFaceCard:title" class="group-title">启动形象</h3>
            <span data-ui="BrandFaceCard:arrow" class="collapse-arrow" aria-hidden="true"></span>
        </summary>
        <div data-ui="BrandFaceCard:body" class="collapse-body">
            <p data-ui="BrandFaceCard:hint" class="muted group-hint">
                自定义开屏头像与左上角图标：可以用当前登录账号的皮肤（皮肤站或正版均可，换号自动跟随），
                也可以上传自己的皮肤文件（64×64 PNG）。
            </p>
            <div v-for="s in slots" :key="s.key" data-ui="BrandFaceCard:row" class="face-row">
                <img
                    data-ui="BrandFaceCard:preview"
                    class="face-preview"
                    :src="preview[s.key]"
                    :alt="s.title + '预览'"
                />
                <div data-ui="BrandFaceCard:main" class="face-main">
                    <strong data-ui="BrandFaceCard:name">{{ s.title }}</strong>
                    <small data-ui="BrandFaceCard:desc" class="muted">{{ s.desc }}</small>
                    <div data-ui="BrandFaceCard:controls" class="face-controls">
                        <select
                            data-ui="BrandFaceCard:source"
                            class="select"
                            :value="sourceOf(s.key)"
                            :disabled="busy[s.key]"
                            :aria-label="s.title + '来源'"
                            @change="
                                onSourceChange(
                                    s.key,
                                    ($event.target as HTMLSelectElement)
                                        .value as FaceSource
                                )
                            "
                        >
                            <option value="default">默认形象</option>
                            <option
                                value="account"
                                :disabled="!store.selectedAccount"
                                :title="
                                    store.selectedAccount
                                        ? '跟随当前登录账号的皮肤'
                                        : '登录账号后可用'
                                "
                            >
                                当前账号皮肤
                            </option>
                            <option value="custom">自定义皮肤…</option>
                        </select>
                        <button
                            v-if="sourceOf(s.key) === 'custom'"
                            data-ui="BrandFaceCard:reupload"
                            class="btn btn-ghost btn-sm"
                            :disabled="busy[s.key]"
                            @click="pickUpload(s.key)"
                        >
                            重新上传
                        </button>
                        <button
                            v-if="sourceOf(s.key) !== 'default'"
                            data-ui="BrandFaceCard:reset"
                            class="btn btn-ghost btn-sm"
                            :disabled="busy[s.key]"
                            @click="onReset(s.key)"
                        >
                            恢复默认
                        </button>
                    </div>
                    <small
                        v-if="!store.selectedAccount"
                        data-ui="BrandFaceCard:noaccount"
                        class="muted face-note"
                        >「当前账号皮肤」需先在账号页登录账号；皮肤站与正版账号都支持。</small
                    >
                </div>
            </div>
            <p v-if="error.splash || error.logo" class="face-error" role="alert">
                {{ error.splash || error.logo }}
            </p>
            <input
                ref="fileInput"
                data-ui="BrandFaceCard:file"
                class="face-file-input"
                type="file"
                accept="image/png"
                @change="onFile"
            />
        </div>
    </details>
</template>

<style scoped>
.face-row {
    display: flex;
    gap: var(--space-4);
    align-items: flex-start;
    padding: var(--space-3) 0;
}
.face-row + .face-row {
    border-top: 1px solid var(--border);
}
.face-preview {
    width: 64px;
    height: 64px;
    border-radius: var(--radius-md);
    image-rendering: pixelated;
    flex-shrink: 0;
    background: var(--card-2);
}
.face-main {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
}
.face-main strong {
    font-size: var(--text-md);
}
.face-main small {
    font-size: var(--text-xs);
    line-height: 1.6;
}
.face-controls {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    flex-wrap: wrap;
    margin-top: var(--space-1);
}
.face-controls .select {
    width: 200px;
    max-width: 100%;
}
.face-note {
    margin-top: var(--space-1);
}
.face-error {
    color: var(--danger);
    font-size: var(--text-xs);
    line-height: 1.6;
}
.face-file-input {
    display: none;
}
</style>
