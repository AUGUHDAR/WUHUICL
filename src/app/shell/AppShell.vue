<script setup lang="ts">
import { inject } from 'vue';
import { APP_SHELL_CONTEXT } from './useAppShell';
import CreatorMotto from '../../components/CreatorMotto.vue';
import AppOverlays from '../overlays/AppOverlays.vue';
const shell = inject(APP_SHELL_CONTEXT)!;
const {
    MascotStage,
    isMac,
    appVersion,
    brandHead,
    mascotOpen,
    mascotReady,
    mascotBusy,
    mascotSoftware,
    mascotRef,
    mascotKeyboard,
    openMascots,
    closeMascots,
    visibleNavItems,
    visibleResourceSubItems,
    resourceExpanded,
    inResourceGroup,
    launcherHealth,
    toggleNotices,
    canGoBack,
    goBack,
    activeTaskCount,
    notesOpen,
    dlOpen,
    store,
    navEl,
    bubbleStyle,
    selectionStyle,
    retargetNav,
    resetNav,
    navFocusOut,
    measureNav,
    onTopbarPointerDown,
    onImportClick,
    win,
    currentComponent,
    routeDuration,
    qqGroup,
    updateModal,
    updatePercent,
    updateSpeedText,
    onUpdateNow,
    onUpdateLater,
    onUpdateSkip,
    onUpdateCancelDownload,
    onUpdateInstallNow,
    configMismatch,
    onConfigReset,
    fmtNoticeTime,
    taskSubText,
    taskEtaText,
    onPauseTask,
    onResumeTask,
    onCancelTask,
    designStageReady,
    bgStyle
} = shell;
</script>

<template>
<Teleport
        :to="designStageReady ? '#design-preview-host' : 'body'"
        :disabled="!designStageReady"
    >
        <!-- 自定义背景层（纯色/图片 + 透明度 + 模糊） -->
        <div data-ui="App:870373af1ab7" v-if="bgStyle" class="app-bg" :style="bgStyle"></div>
        <div
            data-ui="App:e8c1fdc22711"
            class="shell"
            :class="{
                'edit-mode': store.editMode,
                'has-bg': !!bgStyle,
                'mascots-active': mascotOpen,
                'mascots-software': mascotOpen && mascotSoftware
            }"
        >
            <!-- ============ 左侧边栏（宽度 --sidebar-w） ============ -->
            <aside data-ui="App:25064d2bb910" class="sidebar" data-edit="sidebar">
                <!-- Logo 区 -->
                <div data-ui="App:fc5fc8ba7e96" class="logo-area">
                    <div class="brand-slot">
                        <button
                            class="brand-avatar"
                            :class="{
                                'avatar-open': mascotOpen && mascotReady,
                                'avatar-preparing': mascotOpen && !mascotReady
                            }"
                            :tabindex="mascotOpen ? -1 : 0"
                            aria-label="打开 AUGUHDAR 互动彩蛋"
                            :aria-expanded="mascotOpen"
                            :aria-busy="mascotOpen && !mascotReady"
                            :title="
                                mascotOpen && !mascotReady
                                    ? '正在准备 AUGUHDAR…'
                                    : '打开 AUGUHDAR 互动彩蛋'
                            "
                            @click="openMascots"
                        >
                            <img
                                data-ui="App:0f39bd9dbfd2"
                                class="brand-head"
                                :src="brandHead"
                                alt="AUGUHDAR 的 Minecraft 头像"
                            />
                        </button>
                        <MascotStage
                            v-if="mascotOpen"
                            ref="mascotRef"
                            :focus-on-ready="mascotKeyboard"
                            @ready="mascotReady = true"
                            @activity="mascotBusy = $event"
                            @render-mode="mascotSoftware = $event"
                            @close="closeMascots"
                        />
                    </div>
                    <div data-ui="App:7494cda29e47" class="logo-text">
                        <span data-ui="App:c396a9ff34cb" class="logo-name">WUHUICL</span>
                        <span data-ui="App:31accf043a9a" class="logo-version"
                            >v{{ appVersion }}</span
                        >
                    </div>
                </div>

                <!-- 导航 -->
                <nav
                    data-ui="App:adafe645f766"
                    ref="navEl"
                    class="nav"
                    aria-label="主导航"
                    @pointerover="retargetNav"
                    @pointerleave="resetNav"
                    @focusin="retargetNav"
                    @focusout="navFocusOut"
                    @scroll.passive="measureNav"
                >
                    <div
                        data-ui="App:763b7368d7c7"
                        class="nav-bubble"
                        :style="bubbleStyle"
                        aria-hidden="true"
                    ></div>
                    <div class="nav-selection" :style="selectionStyle" aria-hidden="true"></div>
                    <template v-for="item in visibleNavItems" :key="item.key">
                        <button
                            data-ui="App:e2cd389fbd0f"
                            class="nav-item"
                            :data-nav="item.key"
                            :class="{ active: store.currentView === item.key }"
                            :aria-current="store.currentView === item.key ? 'page' : undefined"
                            @click="store.currentView = item.key"
                        >
                            <span
                                data-ui="App:1480740577df"
                                class="nav-icon"
                                v-html="item.icon"
                            ></span>
                            <span data-ui="App:f8a0bfe85bf9" class="nav-label">{{
                                item.label
                            }}</span>
                        </button>

                        <!-- 资源管理子级菜单（插在「游戏版本」之后） -->
                        <template v-if="item.key === 'game'">
                            <button
                                data-ui="App:7080ad4eb8e0"
                                class="nav-item nav-parent"
                                data-nav="resources"
                                :class="{ 'group-active': inResourceGroup }"
                                :aria-expanded="resourceExpanded || inResourceGroup"
                                aria-controls="resource-navigation"
                                @click="resourceExpanded = !resourceExpanded"
                            >
                                <span data-ui="App:48f397d463e5" class="nav-icon">
                                    <svg
                                        viewBox="0 0 24 24"
                                        fill="none"
                                        stroke="currentColor"
                                        stroke-width="2"
                                        stroke-linecap="round"
                                        stroke-linejoin="round"
                                    >
                                        <path
                                            d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"
                                        />
                                        <path d="M3 11h18" />
                                    </svg>
                                </span>
                                <span data-ui="App:215ddb49217f" class="nav-label">资源管理</span>
                                <svg
                                    class="nav-caret"
                                    :class="{ open: resourceExpanded || inResourceGroup }"
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                >
                                    <path d="m9 6 6 6-6 6" />
                                </svg>
                            </button>
                            <!-- 收起时 inert，键盘导航不会落入隐藏的资源菜单。 -->
                            <div
                                data-ui="App:60e2cab1729e"
                                id="resource-navigation"
                                class="nav-sub"
                                :class="{ open: resourceExpanded || inResourceGroup }"
                                :inert="!(resourceExpanded || inResourceGroup)"
                            >
                                <div data-ui="App:23fdfa0f7cd4" class="nav-sub-inner">
                                    <button
                                        data-ui="App:98aaad3e10e8"
                                        v-for="sub in visibleResourceSubItems"
                                        :key="sub.key"
                                        class="nav-item nav-sub-item"
                                        :data-nav="sub.key"
                                        :class="{ active: store.currentView === sub.key }"
                                        :aria-current="
                                            store.currentView === sub.key ? 'page' : undefined
                                        "
                                        @click="store.currentView = sub.key"
                                    >
                                        <span
                                            data-ui="App:1025aa4e9a68"
                                            class="nav-icon"
                                            v-html="sub.icon"
                                        ></span>
                                        <span data-ui="App:6f84d207dfdd" class="nav-label">{{
                                            sub.label
                                        }}</span>
                                    </button>
                                </div>
                            </div>
                        </template>
                    </template>
                </nav>

                <button
                    data-ui="App:f81b37b0f9eb"
                    class="sidebar-health"
                    :class="`is-${launcherHealth.tone}`"
                    title="打开通知中心"
                    @click="toggleNotices"
                >
                    <i data-ui="App:8534bb11ef26"></i>
                    <span data-ui="App:8c2f3c89a0dd">{{ launcherHealth.text }}</span>
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <path d="m9 18 6-6-6-6" />
                    </svg>
                </button>
            </aside>

            <!-- ============ 右侧（顶栏 + 内容） ============ -->
            <div data-ui="App:f900e94b908a" class="main-area">
                <!-- 顶部栏（可拖拽） -->
                <header
                    data-ui="App:db645f1637b0"
                    class="topbar"
                    data-edit="topbar"
                    @pointerdown="onTopbarPointerDown"
                >
                    <button
                        data-ui="App:3cd32ab47022"
                        v-if="canGoBack && store.currentView !== 'home'"
                        class="top-back"
                        :disabled="!canGoBack"
                        title="返回上一个页面"
                        @click="goBack"
                    >
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2.2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        >
                            <path d="m15 18-6-6 6-6" />
                        </svg>
                    </button>
                    <div class="topbar-spacer"><CreatorMotto :disabled="store.editMode" /></div>

                    <div data-ui="App:5f4d42b34aae" class="top-actions">
                        <button
                            data-ui="App:e7efd70d16b8"
                            v-if="store.currentView !== 'home'"
                            class="top-btn dl-toggle"
                            @click="dlOpen = !dlOpen"
                        >
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            >
                                <path d="M12 3v11" />
                                <path d="m7 10 5 5 5-5" />
                                <path d="M4 21h16" />
                            </svg>
                            下载
                            <span
                                data-ui="App:1585a95f110a"
                                v-if="activeTaskCount"
                                class="dl-badge"
                                >{{ activeTaskCount }}</span
                            >
                        </button>
                        <button
                            data-ui="App:0995359279d1"
                            v-if="store.currentView !== 'home'"
                            class="top-btn"
                            @click="onImportClick"
                        >
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            >
                                <path d="M12 15V4" />
                                <path d="m7 8 5-5 5 5" />
                                <path d="M4 15v4a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4" />
                            </svg>
                            导入
                        </button>
                        <button
                            data-ui="App:50549c4d6612"
                            v-if="store.currentView === 'home' && activeTaskCount"
                            class="top-icon-btn dl-toggle"
                            title="下载中心"
                            @click="dlOpen = !dlOpen"
                        >
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            >
                                <path d="M12 3v11" />
                                <path d="m7 10 5 5 5-5" />
                                <path d="M4 21h16" />
                            </svg>
                            <span data-ui="App:34ac91e6ba07" class="dl-badge compact">{{
                                activeTaskCount
                            }}</span>
                        </button>
                        <button
                            data-ui="App:0a28f190d06e"
                            v-if="store.currentView === 'home'"
                            class="top-icon-btn dl-toggle"
                            title="更新日志"
                            @click="notesOpen = !notesOpen"
                        >
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            >
                                <path d="M3 12a9 9 0 1 0 9-9" />
                                <path d="M3 4v5h5" />
                                <path d="M12 7v5l3.5 2" />
                            </svg>
                        </button>
                        <button
                            data-ui="App:b3baec2490ab"
                            class="top-icon-btn"
                            title="通知"
                            @click="toggleNotices"
                        >
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            >
                                <path d="M18 8a6 6 0 1 0-12 0c0 7-3 9-3 9h18s-3-2-3-9" />
                                <path d="M13.7 21a2 2 0 0 1-3.4 0" />
                            </svg>
                            <span
                                data-ui="App:9fb26325a181"
                                v-if="store.noticesUnread"
                                class="bell-dot"
                            ></span>
                        </button>

                        <template v-if="!isMac">
                            <span data-ui="App:2d64ba8e392c" class="top-divider"></span>

                            <button
                                data-ui="App:e3e46a2d0aaf"
                                class="win-btn"
                                title="最小化"
                                @click="win('minimize')"
                            >
                                <svg
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                >
                                    <path d="M5 12h14" />
                                </svg>
                            </button>
                            <button
                                data-ui="App:bd7bf1eb0182"
                                class="win-btn"
                                title="最大化/还原"
                                @click="win('maximize')"
                            >
                                <svg
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                >
                                    <rect x="6" y="6" width="12" height="12" rx="1.5" />
                                </svg>
                            </button>
                            <button
                                data-ui="App:f3cf1f9c86a5"
                                class="win-btn win-close"
                                title="关闭"
                                @click="win('close')"
                            >
                                <svg
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                >
                                    <path d="M6 6l12 12M18 6 6 18" />
                                </svg>
                            </button>
                        </template>
                    </div>

                    <!-- 顶栏下拉统一由 AppOverlays 挂载，保持 teleport 与 DOM 顺序。 -->
                </header>

                <!-- 内容区（:duration 显式给出过渡时长：窗口被遮挡/最小化时 transitionend 不会触发，setTimeout 兜底防切换卡死） -->
                <main data-ui="App:4f03b68b3168" class="content">
                    <Transition name="fade" :duration="routeDuration">
                        <div data-ui="App:ece4739a5266" :key="store.currentView" class="route-view">
                            <component :is="currentComponent" />
                        </div>
                    </Transition>
                </main>
            </div>

        </div>
    </Teleport>

    <AppOverlays />
</template>

<style scoped>
/* Repeated background blur joins unrelated panes to the tiny animated LOGO's
   compositor dependency graph, including native GPU paths. Defer only web blur
   while the interaction is open; theme alpha/colors and native material stay.
   The original glass rules resume immediately when the LOGO is restored. */
.shell.mascots-active,
.shell.mascots-active :deep(*) {
    backdrop-filter: none !important;
    -webkit-backdrop-filter: none !important;
}

.brand-slot {
    position: relative;
    z-index: 5;
    width: 48px;
    height: 72px;
    flex-shrink: 0;
    display: grid;
    place-items: center;
}

.brand-avatar {
    display: block;
    background: transparent;
    border: 0;
    padding: 0;
    border-radius: 12px;
    cursor: pointer;
    transition:
        opacity 0.38s,
        transform 0.38s;
    flex-shrink: 0;
}

.brand-avatar:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 4px;
}

.brand-avatar.avatar-preparing {
    opacity: 0.75;
    transform: scale(0.94);
}

.brand-avatar.avatar-open {
    opacity: 0;
    transform: translateY(-14px) scale(0.65);
    pointer-events: none;
}

.shell {
    display: flex;
    width: 100%;
    height: 100%;
    background: var(--shell-surface);
    position: relative;
    z-index: 1;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--border) 80%, transparent);
    border-radius: var(--radius-md);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.12);
}

/* 自定义背景层：垫底铺满，不拦截交互 */
.app-bg {
    position: fixed;
    inset: 0;
    z-index: 0;
    pointer-events: none;
}

/* 激活自定义背景时界面底色透出背景层 */
.shell.has-bg {
    background: var(--shell-surface);
}

/* ---------------- 左侧边栏 ---------------- */
.sidebar {
    width: var(--sidebar-w);
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    background: color-mix(in srgb, var(--bg-2) 92%, transparent);
    border-right: 1px solid var(--border);
    /* Let Chromium compose the sidebar normally. Forcing a translated,
     backface-hidden surface retains stale navigation pixels after scrolling
     and minimum-window layout changes. The moving highlight keeps its own
     transform and transition below. */
}

.logo-area {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    height: 88px;
    padding: 0 var(--space-4);
    flex-shrink: 0;
}

.brand-head {
    width: 42px;
    height: 42px;
    flex-shrink: 0;
    image-rendering: pixelated;
    border-radius: var(--radius-md);
    border: 1px solid rgba(255, 255, 255, 0.26);
    box-shadow: 0 3px 12px rgba(0, 0, 0, 0.18);
}

.logo-text {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
}

.logo-name {
    font-family: 'Segoe UI Variable Display', 'Segoe UI', sans-serif;
    font-size: var(--text-xl);
    font-weight: 700;
    letter-spacing: 2px;
    line-height: 1.2;
    color: var(--text);
}

.logo-version {
    align-self: flex-end;
    padding-right: 2px;
    font-size: var(--text-xs);
    color: var(--accent-2);
    opacity: 0.85;
}

.nav {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-3) var(--space-3);
    overflow-y: auto;
    position: relative;
}

.nav-item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    height: var(--space-7);
    padding: 0 var(--space-4);
    border: none;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--sidebar-text);
    font-size: var(--text-md);
    font-family: inherit;
    cursor: pointer;
    transition:
        color var(--motion-fast) ease,
        background var(--motion-fast) ease;
    flex-shrink: 0;
    position: relative;
    z-index: 1;
}

.nav-item:hover {
    background: transparent;
    color: var(--text);
}

.nav-item.active {
    background: transparent;
    color: color-mix(in srgb, var(--text) 84%, var(--accent));
}

.nav-bubble {
    position: absolute;
    top: 0;
    left: 0;
    pointer-events: none;
    border-radius: var(--radius-md);
    background: linear-gradient(
        135deg,
        color-mix(in srgb, var(--accent-soft) 80%, transparent),
        var(--hover)
    );
    border: none;
    box-shadow: 0 3px 12px #00000008;
    transition:
        transform 300ms cubic-bezier(0.22, 1, 0.36, 1),
        width 300ms cubic-bezier(0.22, 1, 0.36, 1),
        height 300ms cubic-bezier(0.22, 1, 0.36, 1),
        opacity 120ms ease;
    will-change: transform;
}

.nav-selection {
    position: absolute;
    top: 0;
    left: 0;
    pointer-events: none;
    border-radius: var(--radius-md);
    box-shadow: inset 3px 0 0 var(--accent);
    z-index: 2;
    transition:
        transform 300ms cubic-bezier(0.22, 1, 0.36, 1),
        width 300ms cubic-bezier(0.22, 1, 0.36, 1),
        height 300ms cubic-bezier(0.22, 1, 0.36, 1),
        opacity 120ms ease;
}

.nav-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    flex-shrink: 0;
}

.nav-icon :deep(svg) {
    width: 19px;
    height: 19px;
    display: block;
}

.nav-label {
    font-weight: 500;
}

.nav-item.active .nav-label {
    font-weight: 600;
}

.nav-parent.group-active {
    color: var(--text);
}

/* 资源管理子级菜单 */
.nav-parent .nav-caret {
    width: 14px;
    height: 14px;
    margin-left: auto;
    flex-shrink: 0;
    transition: transform 0.18s ease;
    opacity: 0.7;
}

.nav-parent .nav-caret.open {
    transform: rotate(90deg);
}

/* 子菜单统一展开，避免多次延迟让最后几项迟迟不可用。 */
.nav-sub {
    display: grid;
    grid-template-rows: 0fr;
    opacity: 0;
    margin: 0;
    transition:
        grid-template-rows var(--motion-normal) var(--ease-out),
        opacity 0.22s ease,
        margin var(--motion-normal) var(--ease-out);
}

.nav-sub.open {
    grid-template-rows: 1fr;
    opacity: 1;
    margin: var(--space-1) 0;
}

.nav-sub-inner {
    overflow: hidden;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-height: 0;
}

.nav-sub .nav-sub-item {
    opacity: 0;
    transform: translateY(-4px);
    transition:
        opacity var(--motion-fast) ease,
        transform var(--motion-normal) var(--ease-out);
    transition-delay: 0s;
}

.nav-sub.open .nav-sub-item {
    opacity: 1;
    transform: translateY(0) scale(1);
    transition-delay: 0s;
}

.nav-sub-item {
    height: 40px;
    padding-left: var(--space-6);
    font-size: var(--text-sm);
    position: relative;
}

.nav-sub-item::before {
    content: none;
    position: absolute;
    left: 17px;
    top: 50%;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
    opacity: 0.4;
    transform: translateY(-50%);
}

.nav-sub-item.active::before {
    opacity: 1;
}

.nav-sub-item .nav-icon {
    width: 17px;
    height: 17px;
}

.nav-sub-item .nav-icon :deep(svg) {
    width: 16px;
    height: 16px;
}

.sidebar-health {
    display: grid;
    grid-template-columns: 8px minmax(0, 1fr) 14px;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--row-h);
    margin: var(--space-3);
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--card) 48%, transparent);
    color: var(--text-dim);
    font-family: inherit;
    font-size: var(--text-xs);
    font-weight: 500;
    text-align: left;
    cursor: pointer;
    -webkit-app-region: no-drag;
}

.sidebar-health:hover {
    background: var(--card-2);
    color: var(--text);
}

.sidebar-health i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok);
    box-shadow: 0 0 0 3px var(--ok-soft);
}

.sidebar-health.is-busy i {
    background: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
}

.sidebar-health.is-error i {
    background: var(--danger);
    box-shadow: 0 0 0 3px var(--danger-soft);
}

.sidebar-health span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.sidebar-health svg {
    width: 14px;
    height: 14px;
}

/* ---------------- 右侧区域 ---------------- */
.main-area {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
}

/* 顶部栏 */
.topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    height: 72px;
    flex-shrink: 0;
    padding: 0 var(--space-5);
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg-2) 78%, transparent);
    -webkit-app-region: drag;
}

/* 个性化编辑模式：顶栏不再是拖拽区，点击可落到 DOM 选中顶栏板块（退出编辑恢复拖拽） */
.shell.edit-mode .topbar {
    -webkit-app-region: no-drag;
}

.top-back {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--ctl-h);
    height: var(--ctl-h);
    padding: 0;
    border: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
    border-radius: var(--radius-sm);
    background: var(--card-2);
    color: var(--text-dim);
    cursor: pointer;
    -webkit-app-region: no-drag;
}

.top-back:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--border-strong);
}

.top-back:disabled {
    opacity: 0.62;
    cursor: default;
}

.top-back svg {
    width: 16px;
    height: 16px;
}

.topbar-spacer {
    flex: 1;
    min-width: 0;
    align-self: stretch;
}

@media (max-width: 1050px) {
    .topbar-spacer {
        overflow: hidden;
    }
    .topbar-spacer > .creator-motto {
        visibility: hidden;
    }
    .top-actions {
        flex-shrink: 0;
    }
    .top-actions .top-btn {
        white-space: nowrap;
        flex-shrink: 0;
        padding-inline: 6px;
        gap: 4px;
    }
}

.top-actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    -webkit-app-region: no-drag;
}

.top-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    height: var(--ctl-h);
    padding: 0 var(--space-3);
    border: none;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--text-dim);
    font-size: var(--text-sm);
    font-family: inherit;
    cursor: pointer;
    transition:
        background 0.15s ease,
        color 0.15s ease;
}

.top-btn:hover {
    background: var(--card-2);
    color: var(--accent-2);
}

.top-btn svg {
    width: 15px;
    height: 15px;
}

.top-icon-btn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--ctl-h);
    height: var(--ctl-h);
    border: none;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--text-dim);
    cursor: pointer;
    transition:
        background 0.15s ease,
        color 0.15s ease;
}

.top-icon-btn:hover {
    background: var(--card-2);
    color: var(--text);
}

.top-icon-btn svg {
    width: 17px;
    height: 17px;
}

.bell-dot {
    position: absolute;
    top: 7px;
    right: 8px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    border: 1.5px solid var(--bg-2);
}

.dl-badge.compact {
    position: absolute;
    top: 2px;
    right: 0;
    min-width: 14px;
    height: 14px;
    font-size: var(--text-xs);
}

/* 下载中心 */
.dl-toggle {
    position: relative;
}

.dl-badge {
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: var(--text-xs);
    font-weight: 700;
    display: inline-flex;
    align-items: center;
    justify-content: center;
}

.top-divider {
    width: 1px;
    height: 20px;
    background: var(--border);
    margin: 0 8px;
    flex-shrink: 0;
}

.win-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 38px;
    height: var(--ctl-h);
    border: none;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--text-dim);
    cursor: pointer;
    transition:
        background 0.15s ease,
        color 0.15s ease;
}

.win-btn svg {
    width: 15px;
    height: 15px;
}

.win-btn:hover {
    background: var(--card-2);
    color: var(--text);
}

.win-close:hover {
    background: var(--danger);
    color: #fff;
}

/* 内容区 */
.content {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    scrollbar-gutter: stable;
    padding: var(--space-5);
}

@media (max-width: 1080px) {
    .content {
        padding: var(--space-4);
    }
    .topbar {
        height: 68px;
    }
    .logo-area {
        height: 92px;
    }
}

:global(.btn) {
    min-height: var(--ctl-h);
    padding: 0 var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--card-2);
    color: var(--text);
    font: inherit;
    cursor: pointer;
}
:global(.btn:hover:not(:disabled)) {
    border-color: var(--border-strong);
}
:global(.btn:disabled) {
    opacity: 0.6;
    cursor: default;
}
:global(.btn-sm) {
    min-height: 32px;
    padding: 0 var(--space-3);
    font-size: var(--text-xs);
}
:global(.btn-ghost) {
    background: transparent;
}
:global(.btn-danger) {
    border-color: var(--danger-border);
    color: var(--danger);
}
:global(.btn-gold) {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
}
:global(.muted) {
    color: var(--text-dim);
}
:global(.modal-mask) {
    position: fixed;
    inset: 0;
    z-index: 9500;
    display: grid;
    place-items: center;
    padding: 20px;
    background: var(--mask);
}
:global(.modal) {
    width: min(560px, calc(100vw - 40px));
    max-height: calc(100vh - 40px);
    overflow-y: auto;
    padding: 20px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--card-solid);
    box-shadow: var(--shadow-lg);
}
:global(.modal-title) {
    margin: 0 0 var(--space-3);
    font-size: var(--text-lg);
}
:global(.modal-actions) {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-3);
    margin-top: var(--space-5);
}
</style>
