<script setup lang="ts">
/** 创建房间：把本地已开好的世界广播出去。 */
import { onMounted, ref } from 'vue';
import { call, host, refresh, section, vl } from './useVoxlink';
import './voxlink.css';

const name = ref('');
const password = ref('');
const category = ref('other');
const loader = ref('');
const gameVersion = ref('');
const hostPort = ref(25565);
const visible = ref(true);
const busy = ref(false);
const error = ref('');
const ports = ref<number[]>([]);

onMounted(async () => {
    const r = await call<{ ports?: number[] }>('vlcli:netPorts');
    if (r.ok && Array.isArray(r.data?.ports)) ports.value = r.data.ports as number[];
});

async function submit() {
    if (!name.value.trim()) {
        error.value = '给房间起个名，朋友在大厅里认这个';
        return;
    }
    busy.value = true;
    error.value = '';
    const r = await host({
        name: name.value.trim(),
        password: password.value || undefined,
        category: category.value || undefined,
        loader: loader.value || undefined,
        gameVersion: gameVersion.value || undefined,
        hostPort: hostPort.value,
        visible: visible.value
    });
    busy.value = false;
    if (!r.ok) {
        error.value = r.message || '建房失败';
        return;
    }
    section.value = 'room';
    await refresh();
}
</script>

<template>
    <div class="vl-card" data-ui="voxlink:create">
        <div class="vl-card-head">
            <div>
                <h2>创建房间</h2>
                <p>先在时间里把世界开成对局域网开放，再回来建房。端口填游戏里显示的那个。</p>
            </div>
        </div>

        <label class="vl-field">
            <span>房间名</span>
            <input v-model="name" class="input" maxlength="40" placeholder="例如 生存服，欢迎来砍树" :disabled="busy" />
        </label>

        <div class="vl-row">
            <label class="vl-field" style="flex: 1; min-width: 150px">
                <span>房间密码（可空）</span>
                <input v-model="password" class="input" type="password" :disabled="busy" />
            </label>
            <label class="vl-field" style="width: 120px">
                <span>分类</span>
                <input v-model="category" class="input" placeholder="other" :disabled="busy" />
            </label>
        </div>

        <div class="vl-row">
            <label class="vl-field" style="flex: 1; min-width: 130px">
                <span>加载器</span>
                <input v-model="loader" class="input" placeholder="fabric / forge / vanilla" :disabled="busy" />
            </label>
            <label class="vl-field" style="flex: 1; min-width: 130px">
                <span>游戏版本</span>
                <input v-model="gameVersion" class="input" placeholder="1.20.1" :disabled="busy" />
            </label>
        </div>

        <label class="vl-field">
            <span>本地游戏端口</span>
            <div class="vl-row">
                <input v-model.number="hostPort" class="input" type="number" min="1" max="65535" style="width: 130px" :disabled="busy" />
                <button v-if="ports.length" class="btn btn-ghost btn-sm" :disabled="busy" @click="hostPort = ports[0]">
                    用检测到的 {{ ports.join(' / ') }}
                </button>
            </div>
        </label>

        <label class="vl-row" style="margin-bottom: 4px">
            <input id="vl-visible" v-model="visible" type="checkbox" :disabled="busy" />
            <span class="vl-hint">公开到大厅，陌生人也能搜到</span>
        </label>

        <div v-if="error" class="vl-error">{{ error }}</div>

        <div class="vl-actions">
            <button class="btn" :disabled="busy" @click="submit">
                {{ busy ? '正在建房…' : '创建房间' }}
            </button>
            <span v-if="vl.lastError" class="vl-error" style="margin: 0">{{ vl.lastError }}</span>
        </div>
    </div>
</template>
