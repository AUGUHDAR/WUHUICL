<script setup lang="ts">
import { inject } from 'vue';
import { APP_SHELL_CONTEXT } from '../shell/useAppShell';
import ModpackSupplement from '../../components/ModpackSupplement.vue';
import EditPanel from '../../components/EditPanel.vue';
import ModDropModal from '../../components/ModDropModal.vue';
import WorldImportModal from '../../components/WorldImportModal.vue';
import Toasts from '../../components/Toasts.vue';
import UpdateModal from '../../components/UpdateModal.vue';
import { taskProgressPercent } from '@shared/taskProgress';
import { updateNotes, latestUpdateNote } from '@shared/updateNotes';
import { clearNotices } from '../state';
import { openInstanceCenter } from '../../instanceCenter';
import { formatSpeed } from '../../core/ipc';
import { dismissTask, stageLabel } from '../state';
import SupplementalModsResult from '../../components/SupplementalModsResult.vue';
import LaunchNotice from '../../components/LaunchNotice.vue';
import InstanceCenter from '../../components/InstanceCenter.vue';
const shell = inject(APP_SHELL_CONTEXT)!;
const {
    store,
    noticeOpen,
    fmtNoticeTime,
    notesOpen,
    dlOpen,
    onPauseTask,
    onResumeTask,
    onCancelTask,
    taskSubText,
    taskEtaText,
    activeTaskCount,
    dragActive,
    launchFail,
    onExportLogs,
    closeModpackImport,
    mpModal,
    mpFormatLabel,
    mpFormatTagClass,
    mpRelatedExisting,
    setMpNameSource,
    onMpConflictActionChange,
    mpNeedsReplaceConfirm,
    mpExistingInFolder,
    mpNameConflict,
    onMpTargetFolderChange,
    fmtPackBytes,
    confirmModpackImport,
    modDrop,
    worldModal,
    updateModal,
    qqGroup,
    updatePercent,
    updateSpeedText,
    onUpdateNow,
    onUpdateLater,
    onUpdateSkip,
    onUpdateCancelDownload,
    onUpdateInstallNow,
    configMismatch,
    onConfigReset,
    appVersion,
    taskProgressPercent,
    formatSpeed,
    stageLabel,
    dismissTask
} = shell;
const { instanceCenter } = shell;
</script>

<template>
    <SupplementalModsResult />
    <LaunchNotice />
    <InstanceCenter
        v-if="instanceCenter.target"
        :key="instanceCenter.target.folder + instanceCenter.target.id"
    />
    <!-- 通知中心下拉 -->
                    <Teleport to="body">
                        <div
                            data-ui="App:8e1aef347238"
                            v-if="noticeOpen"
                            class="notice-mask"
                            @click="noticeOpen = false"
                        ></div>
                        <div data-ui="App:9730228f244a" v-if="noticeOpen" class="notice-panel">
                            <div data-ui="App:e6028868bd80" class="notice-head">
                                <span data-ui="App:43feec7b6fd3" class="notice-title">通知</span>
                                <button
                                    data-ui="App:da706702d37c"
                                    class="btn btn-ghost btn-sm"
                                    :disabled="!store.notices.length"
                                    @click="clearNotices"
                                >
                                    清空
                                </button>
                            </div>
                            <div
                                data-ui="App:84df1441fafa"
                                v-if="!store.notices.length"
                                class="notice-empty"
                            >
                                暂无通知
                            </div>
                            <div data-ui="App:b4daafdb8870" v-else class="notice-list">
                                <div
                                    data-ui="App:a73122485a3f"
                                    v-for="n in store.notices"
                                    :key="n.id"
                                    class="notice-item"
                                    :class="'notice-' + n.type"
                                >
                                    <span data-ui="App:adb2057c5de6" class="notice-dot"></span>
                                    <div data-ui="App:2278766c3c3f" class="notice-body">
                                        <p data-ui="App:e0e95443c7b4" class="notice-text">
                                            {{ n.text }}
                                        </p>
                                        <button
                                            v-if="n.exitTarget"
                                            class="btn btn-sm"
                                            @click="
                                                openInstanceCenter(n.exitTarget, 'diagnostics');
                                                noticeOpen = false;
                                            "
                                        >
                                            查看原因
                                        </button>
                                        <span data-ui="App:9f929a457637" class="notice-time">{{
                                            fmtNoticeTime(n.time)
                                        }}</span>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </Teleport>

                    <!-- 更新日志下拉（首页标题栏秒表入口）：内置各版本改动，无网络依赖 -->
                    <Teleport to="body">
                        <div
                            data-ui="App:c07fadcedfba"
                            v-if="notesOpen"
                            class="notice-mask"
                            @click="notesOpen = false"
                        ></div>
                        <div
                            data-ui="App:8e6f6e3d72b8"
                            v-if="notesOpen"
                            class="notice-panel notes-panel"
                        >
                            <div data-ui="App:aeac05a751bf" class="notice-head">
                                <span data-ui="App:6919eb62e967" class="notice-title"
                                    >更新日志</span
                                >
                                <span data-ui="App:632f2221de5a" class="muted"
                                    >{{ latestUpdateNote()?.version }} ·
                                    {{ latestUpdateNote()?.date }}</span
                                >
                            </div>
                            <div data-ui="App:27bd32aa001a" class="notes-list">
                                <section
                                    data-ui="App:f14cd0f7ca63"
                                    v-for="note in updateNotes"
                                    :key="note.version"
                                    class="note-version"
                                >
                                    <h4 data-ui="App:7f4b6a7ad79d" class="note-head">
                                        <span data-ui="App:5860d27f0ee9" class="note-ver">{{
                                            note.version
                                        }}</span>
                                        <span data-ui="App:21f2f31213b2" class="muted">{{
                                            note.date
                                        }}</span>
                                    </h4>
                                    <ul data-ui="App:2ce63f856bef" class="note-changes">
                                        <li
                                            data-ui="App:f7a2ec78fb95"
                                            v-for="(c, i) in note.changes"
                                            :key="i"
                                        >
                                            {{ c }}
                                        </li>
                                    </ul>
                                </section>
                            </div>
                        </div>
                    </Teleport>

                    <!-- 下载中心下拉：版本安装/整合包导入/资源下载统一任务列表，支持取消 -->
                    <Teleport to="body">
                        <div
                            data-ui="App:32e6a4482be2"
                            v-if="dlOpen"
                            class="notice-mask"
                            @click="dlOpen = false"
                        ></div>
                        <div data-ui="App:17d738b19ef5" v-if="dlOpen" class="notice-panel dl-panel">
                            <div data-ui="App:1960026275b2" class="notice-head">
                                <span data-ui="App:9bb15b429862" class="notice-title"
                                    >下载中心</span
                                >
                                <button
                                    data-ui="App:93f5c2a356cd"
                                    class="btn btn-ghost btn-sm"
                                    @click="
                                        dlOpen = false;
                                        store.currentView = 'game';
                                    "
                                >
                                    去版本下载
                                </button>
                            </div>
                            <div
                                data-ui="App:61130e12d278"
                                v-if="!store.tasks.length"
                                class="notice-empty"
                            >
                                没有进行中的任务
                            </div>
                            <div data-ui="App:9f518a7bbb67" v-else class="notice-list">
                                <div
                                    data-ui="App:ac5c4222257f"
                                    v-for="t in store.tasks"
                                    :key="t.id"
                                    :data-task-id="t.id"
                                    class="dl-item"
                                    :class="'dl-' + t.status"
                                >
                                    <div data-ui="App:1147e17bfe61" class="dl-item-head">
                                        <span
                                            data-ui="App:a4f0a4d42456"
                                            class="dl-title"
                                            :title="t.title"
                                            >{{ t.title }}</span
                                        >
                                        <span
                                            data-ui="App:7faad0b50853"
                                            v-if="t.status === 'running'"
                                            class="dl-actions"
                                        >
                                            <button
                                                data-ui="App:4db86123aef2"
                                                class="btn btn-ghost btn-sm"
                                                @click="onPauseTask(t.id)"
                                            >
                                                暂停
                                            </button>
                                            <button
                                                data-ui="App:4f9c2f515cb9"
                                                class="btn btn-ghost btn-sm"
                                                @click="onCancelTask(t.id)"
                                            >
                                                取消
                                            </button>
                                        </span>
                                        <span
                                            data-ui="App:03cdc173529c"
                                            v-else-if="t.status === 'paused'"
                                            class="dl-actions"
                                        >
                                            <button
                                                data-ui="App:7cae05871ea8"
                                                class="btn btn-ghost btn-sm"
                                                @click="onResumeTask(t.id)"
                                            >
                                                继续
                                            </button>
                                            <button
                                                data-ui="App:13a915056e72"
                                                class="btn btn-ghost btn-sm"
                                                @click="onCancelTask(t.id)"
                                            >
                                                取消
                                            </button>
                                        </span>
                                        <button
                                            data-ui="App:ea8947fe4164"
                                            v-else-if="t.status === 'cancelling'"
                                            class="btn btn-ghost btn-sm"
                                            disabled
                                        >
                                            正在取消…
                                        </button>
                                        <button
                                            data-ui="App:d19e1566b2b8"
                                            v-else
                                            class="dl-dismiss"
                                            title="移除记录"
                                            @click="dismissTask(t.id)"
                                        >
                                            ×
                                        </button>
                                    </div>
                                    <div data-ui="App:a1b294f53a4e" class="dl-sub muted">
                                        <template v-if="t.status === 'running'">
                                            {{ taskSubText(t) ? taskSubText(t) + ' · ' : ''
                                            }}{{
                                                t.indeterminate
                                                    ? '正在计算总量'
                                                    : '总进度 ' + taskProgressPercent(t) + '%'
                                            }}{{ taskEtaText(t.etaSeconds) }}
                                            <span v-if="t.speed && t.speed > 0">
                                                · {{ formatSpeed(t.speed) }}</span
                                            >


</template>
                                        <template v-else-if="t.status === 'paused'"
                                            >已暂停 ·
                                            {{
                                                t.indeterminate
                                                    ? '总量未知'
                                                    : taskProgressPercent(t) + '%'
                                            }}</template
                                        >
                                        <template v-else-if="t.status === 'cancelling'"
                                            >正在停止网络与后台任务…</template
                                        >
                                        <template v-else-if="t.status === 'done'">已完成</template>
                                        <template v-else-if="t.status === 'cancelled'"
                                            >已取消</template
                                        >
                                        <template v-else>
                                            失败于「{{ stageLabel(t.stage || 'error') }}」阶段：{{
                                                t.error
                                            }}
                                        </template>
                                    </div>
                                    <ModpackSupplement
                                        v-if="
                                            t.manualFiles &&
                                            (t.status === 'running' || t.status === 'paused')
                                        "
                                        :request="t.manualFiles"
                                        :paused="t.status === 'paused'"
                                    />
                                    <div
                                        data-ui="App:c299fc9739a0"
                                        v-if="
                                            t.status === 'running' ||
                                            t.status === 'paused' ||
                                            t.status === 'cancelling'
                                        "
                                        class="dl-bar"
                                        :class="{
                                            'is-indeterminate':
                                                t.indeterminate && t.status === 'running'
                                        }"
                                    >
                                        <div
                                            data-ui="App:21050ee2a501"
                                            class="dl-bar-fill"
                                            :style="{
                                                width: t.indeterminate
                                                    ? '35%'
                                                    : taskProgressPercent(t) + '%'
                                            }"
                                        ></div>
                                    </div>
                                    <div
                                        data-ui="download.parallel-stages"
                                        v-if="
                                            t.parallelStages?.length &&
                                            (t.status === 'running' || t.status === 'paused')
                                        "
                                        class="dl-stages"
                                    >
                                        <div
                                            v-for="lane in t.parallelStages"
                                            :key="lane.id"
                                            :data-ui="'download.stage.' + lane.id"
                                            class="dl-stage"
                                            :class="{ 'is-done': lane.state === 'done' }"
                                        >
                                            <div class="dl-stage-heading">
                                                <span>{{ lane.label }}</span>
                                                <span class="muted">{{
                                                    lane.state === 'done'
                                                        ? '已就绪'
                                                        : lane.state === 'waiting'
                                                          ? '准备中'
                                                          : lane.indeterminate
                                                            ? '处理中'
                                                            : taskProgressPercent({
                                                                  status: 'running',
                                                                  progress: lane.progress
                                                              }) + '%'
                                                }}</span>
                                            </div>
                                            <div class="dl-stage-detail muted" :title="lane.text">
                                                {{ lane.text }}
                                            </div>
                                            <div v-if="!lane.indeterminate" class="dl-bar">
                                                <div
                                                    class="dl-bar-fill"
                                                    :style="{
                                                        width:
                                                            taskProgressPercent({
                                                                status:
                                                                    lane.state === 'done'
                                                                        ? 'done'
                                                                        : 'running',
                                                                progress: lane.progress
                                                            }) + '%'
                                                    }"
                                                ></div>
                                            </div>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </Teleport>
<!-- 整合包拖入：全屏遮罩（pointer-events:none 保证不干扰拖拽事件） -->
    <Teleport to="body">
        <div data-ui="App:81cc45ec3106" v-if="dragActive" class="drop-mask">
            <div data-ui="App:2a94454a8409" class="drop-box">
                <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                    <path d="m7 8 5-5 5 5" />
                    <path d="M12 3v12" />
                </svg>
                <p data-ui="App:d7601e4e4d82" class="drop-title">
                    松开导入：存档 / 整合包 / MOD / 外置登录提供商
                </p>
            </div>
        </div>
    </Teleport>

    <!-- MOD 拖入即装确认弹窗 -->
    <ModDropModal :open="modDrop.open" :files="modDrop.files" @close="modDrop.open = false" />

    <WorldImportModal
        :open="worldModal.open"
        :file-path="worldModal.filePath"
        :info="worldModal.info"
        @close="worldModal.open = false"
    />

    <!-- 启动失败：提示 + 导出错误日志 -->
    <Teleport to="body">
        <div
            data-ui="App:9d63fadc1ed6"
            v-if="launchFail.open"
            class="modal-mask"
            @pointerdown.self="launchFail.open = false"
        >
            <div data-ui="App:a55c730a5660" class="modal launchfail-modal">
                <h3 data-ui="App:ffddaed1dca5" class="modal-title">{{ launchFail.title }}</h3>
                <p data-ui="App:9ede176a5a84" class="launchfail-text">{{ launchFail.text }}</p>
                <div data-ui="App:b5caea38dc55" class="modal-actions">
                    <button
                        class="btn btn-ghost"
                        @click="
                            openInstanceCenter(
                                { id: store.launchingVersionId, folder: store.launchingFolder },
                                'diagnostics'
                            );
                            launchFail.open = false;
                        "
                    >
                        查看原因
                    </button>
                    <button
                        data-ui="App:bbc871d022c8"
                        class="btn btn-ghost"
                        @click="launchFail.open = false"
                    >
                        关闭
                    </button>
                    <button
                        data-ui="App:43f37fe4338f"
                        class="btn btn-gold"
                        :disabled="launchFail.exporting"
                        @click="onExportLogs"
                    >
                        <svg
                            viewBox="0 0 24 24"
                            width="14"
                            height="14"
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
                        {{ launchFail.exporting ? '导出中…' : '导出错误日志' }}
                    </button>
                </div>
            </div>
        </div>
    </Teleport>

    <!-- 整合包导入确认弹窗 -->
    <Teleport to="body">
        <div
            data-ui="App:597ca4365452"
            v-if="mpModal.open"
            class="modal-mask"
            @pointerdown.self="closeModpackImport"
        >
            <div data-ui="App:cac81d31c8a1" class="modal mp-modal">
                <h3 data-ui="App:df7892dbbb9e" class="mp-title">导入整合包</h3>

                <!-- 解析中 -->
                <div data-ui="App:e3835fac276d" v-if="mpModal.probing" class="mp-loading">
                    <span data-ui="App:f629f7fbfbeb" class="spin"></span>
                    <span data-ui="App:5a2de4468a9a" class="muted">解析中…</span>
                </div>

                <!-- 解析成功：包信息 + 命名选项 -->
                <template v-else-if="mpModal.info">
                    <div data-ui="App:13fbc913792f" class="mp-tags">
                        <span data-ui="App:01e576fd177a" class="tag" :class="mpFormatTagClass">{{
                            mpFormatLabel
                        }}</span>
                        <span data-ui="App:efee1a142b01" class="tag"
                            >MC {{ mpModal.info.mcVersion }}</span
                        >
                        <span
                            data-ui="App:cbc7743772be"
                            v-if="mpModal.info.loader"
                            class="tag tag-gold"
                        >
                            {{ mpModal.info.loader
                            }}{{
                                mpModal.info.loaderVersion ? ' ' + mpModal.info.loaderVersion : ''
                            }}
                        </span>
                        <span
                            data-ui="App:84ba06a0c358"
                            v-if="
                                mpModal.info.version &&
                                mpModal.info.version !== mpModal.info.innerName
                            "
                            class="tag"
                            >版本 {{ mpModal.info.version }}</span
                        >
                    </div>
                    <p data-ui="App:94bfb09f6b96" class="mp-summary">
                        {{ mpModal.info.fileCount }} 个清单文件 ·
                        {{ fmtPackBytes(mpModal.info.downloadBytes) }}
                        <span data-ui="App:b63322e9dda7" v-if="mpModal.info.hasOverrides">
                            · overrides</span
                        >
                        <span data-ui="App:99b02870d974" v-if="mpModal.info.hasClientOverrides">
                            · client-overrides</span
                        >
                    </p>

                    <p data-ui="App:596a49ffb57e" class="mp-label">目标游戏文件夹</p>
                    <select
                        data-ui="App:274944e712ba"
                        v-model="mpModal.targetFolder"
                        class="select"
                        @change="onMpTargetFolderChange"
                    >
                        <option
                            v-for="folder in store.settings?.folders || []"
                            :key="folder.path"
                            :value="folder.path"
                        >
                            {{ folder.name }}{{ folder.isDefault ? '（默认）' : '' }} ·
                            {{ folder.path }}
                        </option>
                    </select>

                    <p data-ui="App:7b097008a36b" class="mp-label">实例命名</p>
                    <div data-ui="App:72a2299fc011" class="mp-name-opts">
                        <button
                            data-ui="App:3a719b866a06"
                            class="mp-name-opt"
                            :class="{ active: mpModal.nameSource === 'file' }"
                            @click="setMpNameSource('file')"
                        >
                            <span data-ui="App:b8da1ddfc48f" class="mp-radio"></span>
                            <span data-ui="App:ffd1edc7712c" class="mp-name-text">
                                <span data-ui="App:46b21df17f0a" class="mp-name-label"
                                    >使用压缩包文件名</span
                                >
                                <span data-ui="App:70d3893f031f" class="mp-name-value">{{
                                    mpModal.info.fileName
                                }}</span>
                            </span>
                        </button>
                        <button
                            data-ui="App:a2e1c9d8bd28"
                            class="mp-name-opt"
                            :class="{ active: mpModal.nameSource === 'inner' }"
                            @click="setMpNameSource('inner')"
                        >
                            <span data-ui="App:ebce50ef2232" class="mp-radio"></span>
                            <span data-ui="App:6471e9d8a5fe" class="mp-name-text">
                                <span data-ui="App:5e013419bf1c" class="mp-name-label"
                                    >使用整合包名称</span
                                >
                                <span data-ui="App:787ec3b759bb" class="mp-name-value">{{
                                    mpModal.info.innerName
                                }}</span>
                            </span>
                        </button>
                    </div>
                    <input
                        data-ui="App:2b301f17a937"
                        v-model="mpModal.customName"
                        class="input mp-custom-name"
                        maxlength="120"
                        placeholder="自定义实例名称"
                        @input="
                            mpModal.confirmReplace = false;
                            mpModal.error = '';
                        "
                    />

                    <div
                        data-ui="App:fc46c9326814"
                        v-if="mpRelatedExisting.length"
                        class="mp-conflict"
                    >
                        <strong data-ui="App:e35ef0415ba0">检测到实例冲突或相同整合包版本</strong>
                        <span data-ui="App:e671a36e2cab">
                            {{
                                mpRelatedExisting
                                    .map(
                                        (item) =>
                                            `${item.id}${item.samePackVersion ? '（同包同版本）' : ''}`
                                    )
                                    .join('、')
                            }}
                        </span>
                        <div data-ui="App:109bf3c86358" class="mp-conflict-actions">
                            <label data-ui="App:f85325e79d42"
                                ><input
                                    data-ui="App:00222a19c9fa"
                                    v-model="mpModal.conflictAction"
                                    type="radio"
                                    value="rename"
                                    @change="onMpConflictActionChange"
                                />
                                重新命名</label
                            >
                            <label data-ui="App:acbdff7eee51"
                                ><input
                                    data-ui="App:614b9af91bd3"
                                    v-model="mpModal.conflictAction"
                                    type="radio"
                                    value="new"
                                    @change="onMpConflictActionChange"
                                />
                                作为新实例安装（自动加序号）</label
                            >
                            <label data-ui="App:89f12da61d4f"
                                ><input
                                    data-ui="App:6dc0ad1f778f"
                                    v-model="mpModal.conflictAction"
                                    type="radio"
                                    value="update"
                                    @change="onMpConflictActionChange"
                                />
                                更新现有实例</label
                            >
                            <label data-ui="App:9a75f39d2361"
                                ><input
                                    data-ui="App:2e73bdfaa685"
                                    v-model="mpModal.conflictAction"
                                    type="radio"
                                    value="overwrite"
                                    @change="onMpConflictActionChange"
                                />
                                覆盖安装</label
                            >
                        </div>

                        <template v-if="mpNeedsReplaceConfirm">
                            <select
                                data-ui="App:831130030ba1"
                                v-model="mpModal.existingId"
                                class="select mp-existing-select"
                            >
                                <option
                                    v-for="item in mpExistingInFolder"
                                    :key="item.id"
                                    :value="item.id"
                                >
                                    {{ item.id
                                    }}{{ item.samePackVersion ? ' · 同一整合包版本' : '' }}
                                </option>
                            </select>
                            <div
                                data-ui="App:5ea11cdf2f61"
                                class="mp-impact"
                                :class="{ danger: mpModal.conflictAction === 'overwrite' }"
                            >
                                <template v-if="mpModal.conflictAction === 'update'">
                                    将重建包管理文件并恢复用户存档、配置及非包管理文件；同名的新包
                                    MOD 优先。操作失败会恢复完整备份。
                                </template>
                                <template v-else>
                                    将重建实例包文件；存档、配置、截图、资源包及可识别的用户 MOD
                                    会保留，其他未知顶层内容可能被移除。操作失败会恢复完整备份。
                                </template>
                            </div>
                            <label data-ui="App:76acdea2ae5a" class="mp-replace-confirm">
                                <input
                                    data-ui="App:8c2c4f6d4693"
                                    v-model="mpModal.confirmReplace"
                                    type="checkbox"
                                />
                                <span data-ui="App:84709e2c73a5"
                                    >我已确认上述影响范围，并同意{{
                                        mpModal.conflictAction === 'update' ? '更新' : '覆盖'
                                    }}所选实例。</span
                                >
                            </label>
                        </template>
                    </div>

                    <!-- 默认按键冲突：检测到作者预设键位且已开启默认按键同步时，给出替换选项（默认不替换） -->
                    <label
                        data-ui="App:0e88d6c50d0d"
                        v-if="mpModal.info?.hasPresetKeys && store.settings?.keySync"
                        class="mp-keysync-opt"
                    >
                        <input
                            data-ui="App:e3245af7e07a"
                            v-model="mpModal.keySyncOverride"
                            type="checkbox"
                        />
                        <span data-ui="App:25de5e67e1b9"
                            >该整合包含作者预设键位（options.txt）。用启动器默认按键替换预设键位；其余设置保留。不勾选则保留作者预设。</span
                        >
                    </label>

                    <p
                        data-ui="App:9feeb075e091"
                        v-if="mpNameConflict && mpModal.conflictAction === 'rename'"
                        class="mp-error"
                    >
                        名称「{{ mpModal.customName }}」已存在，请重新命名或选择其他处理方式。
                    </p>
                    <p data-ui="App:88ee270d21af" v-if="mpModal.error" class="mp-error">
                        {{ mpModal.error }}
                    </p>

                    <div data-ui="App:ca67c0af2cc9" class="mp-actions">
                        <button
                            data-ui="App:20064d25fb9e"
                            class="btn btn-ghost"
                            @click="closeModpackImport"
                        >
                            取消
                        </button>
                        <button
                            data-ui="App:1cef44943a65"
                            class="btn btn-gold"
                            @click="confirmModpackImport"
                        >
                            确认导入
                        </button>
                    </div>
                </template>

                <!-- 解析失败 -->
                <template v-else>
                    <p data-ui="App:597453576b07" class="mp-error">
                        {{ mpModal.error || '无法解析该整合包' }}
                    </p>
                    <div data-ui="App:bea4b4fe18e5" class="mp-actions">
                        <button
                            data-ui="App:4ff5fe62cac3"
                            class="btn btn-ghost"
                            @click="closeModpackImport"
                        >
                            关闭
                        </button>
                    </div>
                </template>
            </div>
        </div>
    </Teleport>

    <Toasts />

    <UpdateModal
        v-if="updateModal.open && updateModal.release"
        :release="updateModal.release"
        :current-version="appVersion"
        :state="updateModal.state"
        :percent="updatePercent"
        :speed-text="updateSpeedText"
        :slow-hint="updateModal.slowHint"
        :qq-group="qqGroup"
        :rollback="updateModal.rollback"
        @update-now="onUpdateNow"
        @later="onUpdateLater"
        @skip="onUpdateSkip"
        @cancel-download="onUpdateCancelDownload"
        @install-now="onUpdateInstallNow"
        @close="updateModal.open = false"
    />

    <div v-if="configMismatch" class="menu-overlay cfg-mismatch-mask">
        <div class="card cfg-mismatch-modal" role="dialog" aria-label="配置不兼容">
            <h3 class="upd-modal-title">配置文件版本不兼容</h3>
            <p class="muted">
                当前配置文件由更新版本的启动器创建，可能包含本版本不认识的格式。可以继续尝试使用（可能异常），或重置为默认设置（原配置会自动备份）。
            </p>
            <div class="upd-modal-actions">
                <button class="btn btn-ghost" @click="configMismatch = false">
                    继续尝试
                </button>
                <button class="btn btn-danger" @click="onConfigReset">
                    重置设置
                </button>
            </div>
        </div>
    </div>

    <EditPanel v-if="store.editMode" />
</template>

<style scoped>
.cfg-mismatch-mask {
    z-index: 9600;
    display: grid;
    place-items: center;
}

.cfg-mismatch-modal {
    width: min(460px, 90vw);
    padding: 20px 22px;
    display: flex;
    flex-direction: column;
    gap: 12px;
}

.upd-modal-title {
    margin: 0;
    font-size: 17px;
}

.upd-modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
}

.notes-list {
    overflow-y: auto;
    padding: 2px var(--space-4) var(--space-4);
}

.note-version + .note-version {
    margin-top: var(--space-3);
    border-top: 1px dashed var(--border);
    padding-top: var(--space-3);
}

.note-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    margin: 0 0 var(--space-2);
}

.note-ver {
    font-weight: 700;
    font-size: var(--text-sm);
}

.note-changes {
    margin: 0;
    padding-left: var(--space-4);
    line-height: 1.7;
    font-size: var(--text-xs);
    color: var(--text);
}

.dl-panel {
    width: 380px;
}

.dl-item {
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--border);
}

.dl-item-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
}

.dl-actions {
    display: inline-flex;
    gap: 4px;
    flex: none;
}

.dl-title {
    font-size: var(--text-sm);
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.dl-sub {
    margin-top: var(--space-1);
    font-size: var(--text-xs);
    line-height: 1.5;
    word-break: break-all;
}

.dl-error .dl-title {
    color: var(--danger);
}

.dl-bar {
    margin-top: var(--space-2);
    height: 5px;
    border-radius: 999px;
    background: var(--card-2);
    overflow: hidden;
}

.dl-bar-fill {
    height: 100%;
    border-radius: 999px;
    background: linear-gradient(90deg, var(--accent-2), var(--accent));
    transition: width 0.3s ease;
}

.dl-stages {
    display: grid;
    gap: 10px;
    margin-top: 14px;
}

.dl-stage {
    min-width: 0;
    padding: 9px 10px;
    border: 1px solid var(--border);
    border-radius: 9px;
    background: var(--card-2);
}

.dl-stage-heading {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: var(--text-xs);
    font-weight: 600;
}

.dl-stage-detail {
    overflow-wrap: anywhere;
    white-space: normal;
    line-height: 1.5;
    font-size: var(--text-xs);
    margin-top: 3px;
}

.dl-stage .dl-bar {
    height: 3px;
    margin-top: 6px;
}

.dl-stage.is-done .dl-stage-heading {
    color: var(--accent);
}

.dl-bar.is-indeterminate .dl-bar-fill {
    animation: dl-indeterminate 1.25s ease-in-out infinite;
}

.dl-dismiss {
    border: none;
    background: transparent;
    color: var(--text-dim);
    font-size: var(--text-md);
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
}

.dl-dismiss:hover {
    color: var(--text);
}

/* 启动失败弹窗 */
.launchfail-modal {
    width: 480px;
}

.launchfail-text {
    font-size: var(--text-sm);
    line-height: 1.7;
    color: var(--text-dim);
    word-break: break-all;
    max-height: 220px;
    overflow-y: auto;
    white-space: pre-wrap;
}

/* ---------------- 整合包拖入遮罩 ---------------- */
.drop-mask {
    position: fixed;
    inset: 0;
    z-index: 9999;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    backdrop-filter: blur(24px) saturate(130%);
    -webkit-backdrop-filter: blur(24px) saturate(130%);
    pointer-events: none; /* 遮罩不拦截拖拽事件，避免 dragleave 闪烁 */
}

.drop-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-7);
    border: 2px dashed var(--accent);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--accent) 8%, var(--card));
    color: var(--accent);
}

.drop-box svg {
    width: 48px;
    height: 48px;
}

.drop-title {
    font-size: var(--text-lg);
    font-weight: 600;
}

/* ---------------- 整合包导入确认弹窗 ---------------- */
.mp-modal {
    width: min(620px, calc(100vw - 40px));
    max-height: 88vh;
    overflow-y: auto;
}

.mp-title {
    font-size: var(--text-lg);
    margin-bottom: var(--space-4);
}

.mp-loading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) 0;
}

.mp-tags {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
}

.mp-summary {
    margin: var(--space-2) 0 0;
    color: var(--text-dim);
    font-size: var(--text-xs);
}

.mp-label {
    font-size: var(--text-sm);
    color: var(--text-dim);
    margin: var(--space-4) 0 var(--space-2);
}

.mp-name-opts {
    display: flex;
    flex-direction: column;
    gap: 8px;
}

.mp-name-opt {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--card-2);
    color: var(--text);
    font-size: var(--text-sm);
    font-family: inherit;
    text-align: left;
    cursor: pointer;
    transition:
        border-color 0.15s ease,
        background 0.15s ease,
        color 0.15s ease;
}

.mp-name-opt:hover {
    border-color: var(--text-dim);
}

.mp-name-opt.active {
    border-color: var(--accent);
    background: var(--accent-soft);
}

.mp-radio {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid var(--text-dim);
    flex-shrink: 0;
    transition:
        border-color 0.15s ease,
        box-shadow 0.15s ease;
}

.mp-name-opt.active .mp-radio {
    border-color: var(--accent);
    box-shadow:
        inset 0 0 0 3px var(--card),
        inset 0 0 0 8px var(--accent);
}

.mp-name-text {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
}

.mp-name-label {
    flex-shrink: 0;
}

.mp-name-value {
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.mp-name-opt.active .mp-name-value {
    color: var(--accent-2);
}

.mp-custom-name {
    width: 100%;
    margin-top: var(--space-2);
}

.mp-conflict {
    display: grid;
    gap: var(--space-2);
    margin-top: var(--space-3);
    padding: var(--space-3);
    border: 1px solid color-mix(in srgb, var(--accent-deep) 48%, var(--border));
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--accent-deep) 8%, var(--card));
    font-size: var(--text-xs);
    color: var(--text-dim);
}

.mp-conflict strong {
    color: var(--text);
}

.mp-conflict-actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-2) var(--space-3);
    margin-top: var(--space-1);
}

.mp-conflict-actions label,
.mp-replace-confirm {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 48px; /* 测试钉死字面量（tests/ui-regressions.test.ts:34），等价 var(--space-7) */
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    cursor: pointer;
    line-height: 1.4;
}

.mp-conflict-actions label:hover,
.mp-replace-confirm:hover {
    background: var(--card-2);
}

.mp-conflict-actions label:has(input:checked),
.mp-replace-confirm:has(input:checked) {
    border-color: var(--accent);
    background: var(--accent-soft);
}

.mp-keysync-opt {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin-top: var(--space-3);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    cursor: pointer;
    line-height: 1.5;
    font-size: var(--text-xs);
    color: var(--text-dim);
}

.mp-keysync-opt:hover {
    background: var(--card-2);
}

.mp-keysync-opt:has(input:checked) {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--text);
}

.mp-keysync-opt input {
    accent-color: var(--accent);
    margin-top: 2px;
    flex-shrink: 0;
}

.mp-conflict input {
    accent-color: var(--accent);
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    margin: 0;
}

.mp-existing-select {
    margin-top: var(--space-1);
}

.mp-impact {
    padding: var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--card-2);
    line-height: 1.55;
}

.mp-impact.danger {
    color: var(--danger);
}

.mp-error {
    font-size: var(--text-sm);
    color: var(--danger);
    line-height: 1.7;
    word-break: break-all;
}

.mp-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-3);
    margin-top: var(--space-5);
}
</style>
