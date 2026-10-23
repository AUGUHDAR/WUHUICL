<script setup lang="ts">
/** 创建房间：实例反查加载器与版本，分类取服务端标签，端口自动检测。 */
import { computed, onMounted, ref } from 'vue';
import { getInstalled } from '../../../api';
import { store } from '../../../store';
import { call, CLIENT_TAG, categories, host, loadCategories, refresh, section, vl } from './useVoxlink';
import './voxlink.css';

const NAME_MAX = 32;
const name = ref('我的世界联机');
const password = ref('');
const visible = ref(true);
const folder = ref('');
const instanceId = ref('');
const manualPort = ref('');
const detectedPorts = ref<number[]>([]);
const options = ref<{ id: string; mcVersion: string; loader: string; folder: string }[]>([]);
const customCategory = ref(false);
const category = ref('survival');
const busy = ref(false);
const error = ref('');
const nameError = ref('');

const chosen = computed(() =>
    options.value.find((o) => o.id === instanceId.value && o.folder === folder.value) || null
);

function defaultFolder(): string {
    const s = store.settings as unknown as Record<string, unknown> | undefined;
    const folders = (s?.folders || []) as { path: string; isDefault?: boolean }[];
    return String(s?.activeFolder || folders.find((f) => f.isDefault)?.path || '');
}

async function loadInstances() {
    const list = await getInstalled(true);
    options.value = list
        .filter((v) => !v.incomplete && !v.failed)
        .map((v) => ({ id: v.id, mcVersion: v.mcVersion, loader: v.loader || 'vanilla', folder: v.folder }));
    folder.value = defaultFolder();
    const hit =
        options.value.find((o) => o.id === store.resourceVersionId && o.folder === folder.value) ||
        options.value.find((o) => o.folder === folder.value) ||
        options.value[0];
    if (hit) {
        instanceId.value = hit.id;
        folder.value = hit.folder;
    }
}

function selectInstance(id: string) {
    const hit = options.value.find((o) => o.id === id && o.folder === folder.value) || options.value.find((o) => o.id === id);
    if (hit) {
        instanceId.value = hit.id;
        folder.value = hit.folder;
    }
}

async function detectPorts() {
    const r = await call<{ ports?: number[] }>('vlcli:netPorts');
    if (r.ok && Array.isArray(r.data?.ports)) detectedPorts.value = r.data.ports as number[];
}

function pickCategory(value: string) {
    customCategory.value = false;
    category.value = value;
}

async function submit() {
    const trimmed = name.value.trim();
    nameError.value = '';
    error.value = '';
    if (!trimmed) {
        nameError.value = '请输入房间名';
        return;
    }
    if (trimmed.length > NAME_MAX) {
        nameError.value = '房间名不能超过 32 个字符';
        return;
    }
    if (!instanceId.value || !folder.value) {
        error.value = '请先选择房主正在使用的游戏实例';
        return;
    }
    const port = manualPort.value.trim();
    if (port && (Number(port) < 1 || Number(port) > 65535)) {
        error.value = '请填写游戏中显示的有效端口（1–65535）';
        return;
    }
    busy.value = true;
    const r = await host({
        name: trimmed,
        password: password.value || undefined,
        category: category.value,
        visible: visible.value,
        hostPort: port ? Number(port) : undefined,
        clientTag: CLIENT_TAG,
        target: { folder: folder.value, id: instanceId.value },
        loader: chosen.value?.loader,
        gameVersion: chosen.value?.mcVersion
    });
    busy.value = false;
    if (!r.ok) {
        error.value = r.message || '建房失败';
        return;
    }
    section.value = 'room';
    await refresh();
}

onMounted(() => {
    void loadInstances();
    void loadCategories();
    void detectPorts();
});
</script>

<template>
    <div class="vl-card" data-ui="voxlink:create">
        <div class="vl-card-head">
            <div>
                <h2>创建房间</h2>
                <p>先进游戏把世界对局域网开放，再回来建房。房号发给朋友就能连进来。</p>
            </div>
        </div>

        <label class="vl-field">
            <span>房间名</span>
            <input v-model="name" class="input" :maxlength="NAME_MAX" :disabled="busy" />
            <span v-if="nameError" class="vl-error" style="margin: 0">{{ nameError }}</span>
        </label>

        <div class="vl-field">
            <span>分类</span>
            <div class="vl-row">
                <button
                    v-for="(label, value) in categories"
                    :key="value"
                    class="vl-pill"
                    :class="{ ok: !customCategory && category === value }"
                    :disabled="busy"
                    @click="pickCategory(value)"
                >
                    {{ label }}
                </button>
                <button class="vl-pill" :class="{ ok: customCategory }" :disabled="busy" @click="customCategory = true">
                    自定义
                </button>
            </div>
            <input
                v-if="customCategory"
                v-model="category"
                class="input"
                maxlength="16"
                placeholder="自己填一个标签"
                :disabled="busy"
            />
        </div>

        <div class="vl-row">
            <label class="vl-field" style="flex: 1; min-width: 240px">
                <span>游戏实例</span>
                <select class="input" :value="instanceId" :disabled="busy" @change="selectInstance(($event.target as HTMLSelectElement).value)">
                    <option v-for="o in options" :key="`${o.folder}/${o.id}`" :value="o.id">
                        {{ o.id }}（{{ o.mcVersion }} · {{ o.loader }}）
                    </option>
                </select>
            </label>
            <label class="vl-field" style="width: 180px">
                <span>房间密码（可空）</span>
                <input v-model="password" class="input" type="password" :disabled="busy" />
            </label>
        </div>

        <p class="vl-hint">
            加载器与版本按选中实例上报：{{ chosen ? `${chosen.mcVersion} · ${chosen.loader}` : '还没选到实例' }}
        </p>

        <label class="vl-row" style="margin: 12px 0">
            <input v-model="visible" type="checkbox" :disabled="busy" />
            <span class="vl-hint">在公共大厅显示。关闭后，好友仍可凭房间码加入。</span>
        </label>

        <details class="vl-field">
            <summary class="vl-hint">没有检测到游戏？手动填写端口</summary>
            <div class="vl-row" style="margin-top: 8px">
                <input
                    v-model="manualPort"
                    class="input"
                    style="width: 130px"
                    :placeholder="String(detectedPorts[0] || 25565)"
                    :disabled="busy"
                />
                <button class="btn btn-ghost btn-sm" :disabled="busy" @click="detectPorts">重新检测</button>
            </div>
        </details>
        <p class="vl-hint">
            {{ detectedPorts.length ? `检测到游戏在听 ${detectedPorts.join(' / ')}` : '没检测到端口，留空按游戏默认上报' }}
        </p>

        <div v-if="error" class="vl-error">{{ error }}</div>
        <div v-if="vl.lastError" class="vl-error">{{ vl.lastError }}</div>

        <div class="vl-actions">
            <button class="btn" :disabled="busy" @click="submit">{{ busy ? '正在建房…' : '创建房间' }}</button>
        </div>
    </div>
</template>
