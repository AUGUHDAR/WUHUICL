<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, onMounted, ref, watch } from 'vue';
import {
    applyLocalUpdate,
    applyPendingUpdate,
    checkUpdate,
    errText,
    getPendingUpdate,
    getUpdateState,
    listUpdateReleases,
    pickLocalUpdateFile,
    restoreUpdateBackup
} from '../../../api';
import { store, toast } from '../../../store';
import type { LocalUpdateCheck, ReleaseInfo, UpdateStateInfo } from '@shared/types';
import {
    settingsCatalog,
    settingsCategories,
    settingsScopes,
    scopeOfCategory,
    searchSettings,
    type SettingsCategory
} from '@shared/settingsCatalog';
import { useMotion } from '../../../motion';
import { usePlatformUpdate } from '../../updates/data';
import { AppearanceThemeCard, HomeLayoutEditor } from '../../appearance';
import SettingsFeaturesSection from './SettingsFeaturesSection.vue';
import SettingsDownloadsSection from './SettingsDownloadsSection.vue';
import SettingsRuntimeSection from './SettingsRuntimeSection.vue';
import SettingsDisplaySection from './SettingsDisplaySection.vue';
import SettingsLaunchSection from './SettingsLaunchSection.vue';
import SettingsUpdatesSection from './SettingsUpdatesSection.vue';
import SettingsPluginsSection from './SettingsPluginsSection.vue';
import SettingsUpdateDialogs from './SettingsUpdateDialogs.vue';
import { useSettingsPersistence } from './useSettingsPersistence';
const ThirdPartyNotices = defineAsyncComponent(() => import('../../updates/ui/ThirdPartyNotices.vue'));

const page = ref<HTMLElement | null>(null);
const { systemReduced } = useMotion();
const { systemInstaller, installAction, updateReadyMessage } = usePlatformUpdate();
const { save } = useSettingsPersistence();
const appVersion = __APP_VERSION__;
const systemMotionHelp =
    window.kamucl.platform === 'darwin'
        ? '若需要动画，请在系统设置 → 辅助功能 → 显示中关闭“减少动态效果”。'
        : window.kamucl.platform === 'win32'
          ? '若需要动画，请在 Windows 设置 → 辅助功能 → 视觉效果开启动画效果。'
          : '若需要动画，请检查本机桌面的辅助功能或动画设置。';

const savedCategory = sessionStorage.getItem('kamucl.settings.category');
const category = ref<SettingsCategory>(
    settingsCategories.find((item) => item.id === savedCategory)?.id ?? 'appearance'
);
const settingsQuery = ref('');
const searchMatches = computed(() => searchSettings(settingsQuery.value));
const scope = computed(() => scopeOfCategory(category.value));
const visibleCategories = computed(() =>
    settingsCategories.filter((item) => item.scope === scope.value)
);
const lastCategories: Record<'launcher' | 'game', SettingsCategory> = {
    launcher: 'appearance',
    game: 'game'
};
lastCategories[scope.value] = category.value;
function selectScope(id: 'launcher' | 'game') {
    void selectCategory(lastCategories[id]);
}
const positions = new Map<string, number>();
let navigation = 0;
function scroller() {
    return page.value?.querySelector<HTMLElement>('.settings-body');
}
async function selectCategory(id: SettingsCategory, section?: string) {
    const ticket = ++navigation;
    const scroll = scroller();
    positions.set(category.value, scroll?.scrollTop ?? 0);
    category.value = id;
    settingsQuery.value = '';
    sessionStorage.setItem('kamucl.settings.category', id);
    await nextTick();
    if (ticket !== navigation || !scroll) return;
    scroll.scrollTop = Math.min(
        positions.get(id) ?? 0,
        Math.max(0, scroll.scrollHeight - scroll.clientHeight)
    );
    if (section) await revealSetting(section);
}
async function jumpSetting(id: string) {
    const item = settingsCatalog.find((entry) => entry.id === id);
    if (!item) return;
    await selectCategory(item.category, item.id);
}
async function revealSetting(id: string) {
    const item = settingsCatalog.find((entry) => entry.id === id);
    if (!item) return;
    const target = page.value?.querySelector<HTMLElement>('[data-section="' + id + '"]');
    if (!target) return;
    if (target.tagName === 'DETAILS') (target as HTMLDetailsElement).open = true;
    for (
        let parent = target.parentElement;
        parent && parent !== scroller();
        parent = parent.parentElement
    ) {
        if (parent.tagName === 'DETAILS') (parent as HTMLDetailsElement).open = true;
    }
    await nextTick();
    target.setAttribute('tabindex', '-1');
    const body = scroller();
    if (body)
        body.scrollTop +=
            target.getBoundingClientRect().top - body.getBoundingClientRect().top - 12;
    target.focus({ preventScroll: true });
}
async function revealSection() {
    if (!store.settings || !store.settingsSection) return;
    const id = store.settingsSection;
    store.settingsSection = '';
    const item = settingsCatalog.find((entry) => entry.id === id);
    if (!item) return;
    await selectCategory(item.category, id);
}
onMounted(revealSection);
watch([() => store.settingsSection, () => !!store.settings], revealSection, { flush: 'post' });

async function changeWindowFit(event: Event) {
    const input = event.target as HTMLInputElement;
    const previous = store.settings?.uiWindowAutoFit === true;
    const inputValue = input.checked;
    input.disabled = true;
    const saved = await save(
        { uiWindowAutoFit: inputValue },
        '保存窗口自适应失败：'
    );
    input.disabled = false;
    if (!saved) input.checked = previous;
}

const showLicenses = ref(false);
const updateCheckState = ref<'idle' | 'checking' | 'latest' | 'failed'>('idle');
const lastManualCheck = ref(0);
const rollback = ref<{ open: boolean; loading: boolean; list: ReleaseInfo[]; selected: string }>({
    open: false,
    loading: false,
    list: [],
    selected: ''
});
const updateState = ref<UpdateStateInfo | null>(null);
const pendingUpdate = ref<{ release: ReleaseInfo; file: string } | null>(null);
const restoringBackup = ref(false);
const localUpdate = ref<{ check: LocalUpdateCheck; confirming: boolean } | null>(null);

async function onCheckUpdate() {
    const now = Date.now();
    if (now - lastManualCheck.value < 5 * 60_000 && updateCheckState.value === 'latest') {
        toast('5 分钟内已检查过，已是最新', 'info');
        return;
    }
    lastManualCheck.value = now;
    updateCheckState.value = 'checking';
    try {
        const result = await checkUpdate(true);
        if (result.ok && result.hasUpdate && result.release) {
            updateCheckState.value = 'idle';
            store.updatePrompt = { release: result.release, rollback: false };
        } else if (result.ok) updateCheckState.value = 'latest';
        else {
            updateCheckState.value = 'failed';
            toast('检查失败：断网或更新源不可达（已记日志）', 'error');
        }
    } catch {
        updateCheckState.value = 'failed';
        toast('检查失败：断网或更新源不可达（已记日志）', 'error');
    }
}
async function openRollback() {
    rollback.value.open = true;
    rollback.value.loading = true;
    try {
        rollback.value.list = (await listUpdateReleases()).filter(
            (release) => release.version !== appVersion
        );
        if (!rollback.value.list.length) toast('没有可回退的历史版本', 'info');
    } catch (error) {
        toast('获取历史版本失败：' + errText(error), 'error');
    } finally {
        rollback.value.loading = false;
    }
}
function confirmRollback() {
    const release = rollback.value.list.find(
        (item) => item.version === rollback.value.selected
    );
    rollback.value.open = false;
    if (release) store.updatePrompt = { release, rollback: true };
}
function releaseSummary(body: string): string {
    const first =
        (body || '')
            .split(/\r?\n/)
            .map((line) => line.replace(/^(?:#+|[-*])\s*/, '').trim())
            .filter(
                (line) => line && !/^(?:KAMUCL|WUHUICL)\s+v?[\d.]+$/i.test(line)
            )[0] ?? '';
    return first.length > 60 ? first.slice(0, 60) + '…' : first;
}
function releaseDate(value: string): string {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return '';
    const pad = (number: number) => String(number).padStart(2, '0');
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
async function refreshUpdateState() {
    try {
        updateState.value = await getUpdateState();
    } catch {
        updateState.value = null;
    }
    try {
        pendingUpdate.value = await getPendingUpdate();
    } catch {
        pendingUpdate.value = null;
    }
}
async function onRestoreBackup() {
    if (!updateState.value) return;
    restoringBackup.value = true;
    try {
        await restoreUpdateBackup();
        restoringBackup.value = false;
        await refreshUpdateState();
        toast('备份已准备，下次手动启动时恢复', 'success');
    } catch (error) {
        restoringBackup.value = false;
        toast('还原失败：' + errText(error), 'error');
    }
}
async function onApplyPending() {
    try {
        await applyPendingUpdate();
        toast(updateReadyMessage(), 'success');
    } catch (error) {
        toast('安装失败：' + errText(error), 'error');
    }
}
async function onPickLocalUpdate() {
    try {
        const check = await pickLocalUpdateFile();
        if (check) localUpdate.value = { check, confirming: true };
    } catch (error) {
        toast('校验安装包失败：' + errText(error), 'error');
    }
}
async function confirmLocalUpdate() {
    const update = localUpdate.value;
    if (!update) return;
    localUpdate.value = null;
    try {
        await applyLocalUpdate(update.check);
        await refreshUpdateState();
        toast(updateReadyMessage('本地更新已准备'), 'success');
    } catch (error) {
        toast('安装更新失败：' + errText(error), 'error');
    }
}
onMounted(refreshUpdateState);
</script>

<template>
    <div data-ui="SettingsView:bf6b46a9d4d6" ref="page" class="page settings-page">
        <div data-ui="SettingsView:4348bca0a0cd" class="page-head">
            <h1 data-ui="SettingsView:23c5aeacd581" class="page-title">设置</h1>
        </div>
        <div data-ui="SettingsView:821acd5b45d3" class="settings-navigation">
            <label data-ui="SettingsView:213f2c95d295" class="settings-search">
                <input
                    data-ui="SettingsView:a49601b2b97e"
                    v-model="settingsQuery"
                    class="input"
                    type="search"
                    placeholder="搜索：内存、Java、动画、下载…"
                    aria-label="搜索设置"
                    @keydown.esc="settingsQuery = ''"
                />
            </label>
            <nav
                data-ui="SettingsView:f6863d2b3382"
                class="settings-scopes"
                aria-label="设置范围"
            >
                <button
                    data-ui="SettingsView:5b5d344ab29c"
                    v-for="item in settingsScopes"
                    :key="item.id"
                    class="btn btn-ghost"
                    :aria-current="scope === item.id ? 'page' : undefined"
                    @click="selectScope(item.id)"
                >
                    {{ item.label }}
                </button>
            </nav>
            <nav
                data-ui="SettingsView:59cf14ccb4be"
                class="settings-categories"
                aria-label="设置分类"
            >
                <button
                    data-ui="SettingsView:68ab6a2985c0"
                    v-for="item in visibleCategories"
                    :key="item.id"
                    class="btn btn-ghost"
                    :aria-current="category === item.id ? 'page' : undefined"
                    @click="selectCategory(item.id)"
                >
                    {{ item.label }}
                </button>
            </nav>
            <div
                data-ui="SettingsView:9e789d8ad646"
                v-if="settingsQuery.trim()"
                class="settings-search-results"
                role="region"
                aria-label="设置搜索结果"
            >
                <p
                    data-ui="SettingsView:4c05037c5017"
                    v-if="!searchMatches.length"
                    class="muted"
                >
                    没有找到相关设置，试试“主题”“内存”或“下载”。
                </p>
                <button
                    data-ui="SettingsView:5bdb3d2f1359"
                    v-for="item in searchMatches"
                    :key="item.id"
                    class="btn btn-ghost"
                    @click="jumpSetting(item.id)"
                >
                    <strong data-ui="SettingsView:32a83676fd73">{{ item.name }}</strong>
                    <span data-ui="SettingsView:82dbc5932250" class="muted">
                        {{
                            settingsScopes.find(
                                (scopeItem) =>
                                    scopeItem.id === scopeOfCategory(item.category)
                            )?.label
                        }}
                        ·
                        {{ settingsCategories.find((entry) => entry.id === item.category)?.label }}
                        →
                    </span>
                </button>
            </div>
        </div>
        <div
            data-ui="SettingsView:3f9566ff298f"
            class="settings-body"
            tabindex="0"
            aria-label="设置内容"
        >
            <div data-ui="SettingsView:a82a0d33446b" v-if="!store.settings" class="card empty">
                <span data-ui="SettingsView:97a8ccc98948" class="spin"></span>
                <span data-ui="SettingsView:65c5c2524d71">正在加载设置…</span>
            </div>
            <template v-else>
                <AppearanceThemeCard v-show="category === 'appearance'" />
                <SettingsFeaturesSection :visible="category === 'features'" />
                <div
                    data-ui="SettingsView:9816c9c5870a"
                    class="background-settings"
                    v-show="category === 'appearance'"
                >
                    <HomeLayoutEditor />
                </div>
                <div
                    data-ui="SettingsView:6e353ae31f3c"
                    v-if="category === 'appearance'"
                    class="card group group-inline setting-target"
                    data-section="ui-window-fit"
                    tabindex="-1"
                >
                    <div data-ui="SettingsView:a01c31d661f3">
                        <h3 data-ui="SettingsView:7a11978537e7" class="group-title">UI 窗口自适应</h3>
                        <p data-ui="SettingsView:0c4e326f9e92" class="muted">
                            默认关闭。开启后按当前屏幕的可用区域调整窗口，并在较小窗口内适当缩小界面。适用于
                            1366 × 768 等小屏幕；不修改显示器分辨率、图片或游戏画质。
                        </p>
                        <p data-ui="SettingsView:101d2050bfeb" class="muted">
                            手动缩放仍可使用，关闭后恢复手动缩放比例。
                        </p>
                    </div>
                    <label data-ui="SettingsView:413b509fd853" class="switch">
                        <input
                            data-ui="SettingsView:1cfe8caa5942"
                            type="checkbox"
                            aria-label="UI 窗口自适应"
                            :checked="store.settings.uiWindowAutoFit === true"
                            @change="changeWindowFit"
                        />
                        <span data-ui="SettingsView:990eeb2df557" class="switch-ui"></span>
                    </label>
                </div>
                <div
                    data-ui="SettingsView:0683ad7389b1"
                    v-if="category === 'appearance'"
                    class="card group group-inline setting-target"
                    data-section="motion"
                    tabindex="-1"
                >
                    <div data-ui="SettingsView:8464380631ed">
                        <h3 data-ui="SettingsView:c37223f9bda7" class="group-title">
                            减少动态效果
                        </h3>
                        <p data-ui="SettingsView:53f72432b671" class="muted">
                            停止装饰动画与自动轮播，缩短过渡。系统开启减少动态效果时也会自动生效。
                        </p>
                        <p
                            data-ui="SettingsView:1182a4d00aa1"
                            v-if="systemReduced"
                            class="muted"
                            role="status"
                        >
                            当前系统已关闭动画：启动时显示完整头像，皮肤保持站姿。{{ systemMotionHelp }}
                        </p>
                    </div>
                    <label data-ui="SettingsView:372d19fc52db" class="switch">
                        <input
                            data-ui="SettingsView:35ef39b7e9bc"
                            type="checkbox"
                            aria-label="减少动态效果"
                            :checked="store.settings.reduceMotion === true"
                            @change="
                                save({
                                    reduceMotion: ($event.target as HTMLInputElement)
                                        .checked
                                })
                            "
                        />
                        <span data-ui="SettingsView:4490d3e5d395" class="switch-ui"></span>
                    </label>
                </div>
                <SettingsDownloadsSection :visible="category === 'downloads'" />
                <div
                    data-ui="SettingsView:72c1d8b58f43"
                    v-show="category === 'directories'"
                    data-section="isolation"
                    class="card group group-inline"
                >
                    <div data-ui="SettingsView:1315e5bfd585">
                        <h3 data-ui="SettingsView:a0ad2fcbf796" class="group-title">
                            新版本默认开启版本隔离（推荐）
                        </h3>
                        <p data-ui="SettingsView:54184620a500" class="muted group-hint">
                            仅影响新安装的版本。开启后独立保存存档、模组与配置；已有实例在游戏版本页调整。
                        </p>
                    </div>
                    <label data-ui="SettingsView:7ffae0fe364c" class="switch">
                        <input
                            data-ui="SettingsView:7af59c7c71ed"
                            type="checkbox"
                            :checked="store.settings.defaultIsolation"
                            @change="
                                save({
                                    defaultIsolation: ($event.target as HTMLInputElement)
                                        .checked
                                })
                            "
                        />
                        <span data-ui="SettingsView:a6805d615f04" class="switch-ui"></span>
                    </label>
                </div>
                <SettingsRuntimeSection :visible="category === 'game'" />
                <SettingsDisplaySection :visible="category === 'display'" />
                <SettingsLaunchSection :visible="category === 'general'" />
                <SettingsUpdatesSection
                    :visible="category === 'about'"
                    :app-version="appVersion"
                    :check-state="updateCheckState"
                    :pending-update="pendingUpdate"
                    :update-state="updateState"
                    :restoring-backup="restoringBackup"
                    :system-installer="systemInstaller"
                    :install-action="installAction"
                    :update-ready-message="updateReadyMessage"
                    @check="onCheckUpdate"
                    @licenses="showLicenses = true"
                    @apply-pending="onApplyPending"
                    @rollback="openRollback"
                    @restore-backup="onRestoreBackup"
                    @pick-local="onPickLocalUpdate"
                    @auto-update="save({ autoUpdate: $event })"
                />
                <SettingsPluginsSection :visible="category === 'features'" />
            </template>
        </div>
        <SettingsUpdateDialogs
            :app-version="appVersion"
            :rollback="rollback"
            :local-update="localUpdate"
            :release-summary="releaseSummary"
            :release-date="releaseDate"
            @close-rollback="rollback.open = false"
            @close-local="localUpdate = null"
            @confirm-rollback="confirmRollback"
            @confirm-local="confirmLocalUpdate"
        />
        <ThirdPartyNotices v-if="showLicenses" @dismiss="showLicenses = false" />
    </div>
</template>

<style scoped>
.settings-page {
    height: 100%;
    min-height: 0;
    display: grid !important;
    grid-template-columns: minmax(0, 1fr) minmax(220px, 340px);
    grid-template-rows: auto auto auto minmax(0, 1fr);
    gap: 10px !important;
    padding: 0 !important;
    max-width: 1080px !important;
    position: relative;
}
.settings-page > .page-head {
    grid-row: 1;
    grid-column: 1;
    margin: 0 !important;
    align-self: center;
}
.settings-navigation {
    display: contents;
}
.settings-search {
    grid-column: 2;
    grid-row: 1;
    min-width: 0;
}
.settings-search input {
    width: 100%;
}
.settings-scopes {
    grid-column: 1/-1;
    grid-row: 2;
    display: flex;
    gap: 6px;
}
.settings-scopes .btn {
    font-size: 15px;
    font-weight: 650;
    min-height: 38px;
    padding: 8px 18px;
    border-color: transparent;
}
.settings-scopes [aria-current] {
    background: var(--accent-soft);
    color: var(--accent-2);
}
.settings-categories {
    grid-column: 1/-1;
    grid-row: 3;
    margin: 0;
    border-bottom: 1px solid var(--border);
    padding: 0 0 8px;
    gap: 4px;
}
.settings-categories .btn {
    border: 0;
    border-radius: var(--radius-sm);
    min-height: 32px;
    padding: 6px 12px;
    font-size: 13px;
}
.settings-search-results {
    position: absolute;
    right: 0;
    top: 48px;
    z-index: 20;
    width: min(480px, 100%);
    max-height: 60vh;
    overflow: auto;
    padding: 12px;
    background: var(--surface-solid);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow);
}
.settings-body {
    grid-column: 1/-1;
    grid-row: 4;
    min-height: 0;
    overflow: auto;
    scrollbar-gutter: stable;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 2px 8px 20px 0;
}
.settings-body > * {
    flex-shrink: 0;
}
.settings-body :deep(.card.group) {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: 14px 18px;
}
.settings-body :deep(.collapse) {
    padding: 0;
    gap: 0;
}
.settings-body :deep(.collapse-body) {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 18px 14px;
    border-top: 1px solid var(--border);
}
.settings-body :deep(.collapse-head) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    min-height: 42px;
    padding: 10px 18px;
    cursor: pointer;
    list-style: none;
    user-select: none;
}
.settings-body :deep(.collapse-head::-webkit-details-marker) {
    display: none;
}
.settings-body :deep(.collapse-arrow) {
    width: 7px;
    height: 7px;
    border-right: 2px solid var(--text-dim);
    border-bottom: 2px solid var(--text-dim);
    transform: rotate(-45deg);
    transition: transform 0.18s ease;
}
.settings-body :deep(.collapse[open] > .collapse-head .collapse-arrow) {
    transform: rotate(45deg);
}
.settings-body :deep(.group-title) {
    font-size: 15px;
    margin-bottom: 8px;
}
.settings-body :deep(.collapse-head .group-title) {
    margin: 0;
}
.settings-body :deep(.group-hint) {
    font-size: 12px;
    line-height: 1.6;
    margin: 6px 0;
}
.settings-body :deep(.group-inline) {
    align-items: center;
    gap: 16px;
    flex-direction: row;
    justify-content: space-between;
}
.settings-body :deep(.group-inline > div) {
    flex: 1;
    min-width: 0;
}
.settings-body :deep(.group-inline .switch) {
    flex: none;
}
.settings-body [data-section='motion'] {
    padding: 12px 18px;
    order: 1;
}
.settings-body [data-section='motion'] p {
    font-size: 12px;
    line-height: 1.6;
    margin: 0;
}
.background-settings {
    order: 2;
    display: grid;
    gap: 12px;
}
.settings-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    align-items: start;
    gap: 12px;
}
.settings-grid > .card {
    height: auto;
}
[data-section] {
    scroll-margin-top: 12px;
}
[data-section]:focus {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
}
@media (max-width: 800px) {
    .settings-page {
        grid-template-columns: minmax(0, 1fr);
        grid-template-rows: auto auto auto minmax(0, 1fr);
    }
    .settings-page > .page-head {
        grid-column: 1;
    }
    .settings-search {
        grid-column: 1;
        grid-row: 2;
    }
    .settings-scopes {
        grid-row: 3;
    }
    .settings-categories {
        grid-row: 4;
        overflow-x: auto;
    }
    .settings-body {
        grid-row: 5;
    }
    .settings-body :deep(.group-inline) {
        align-items: flex-start;
        flex-direction: column;
    }
}
</style>
