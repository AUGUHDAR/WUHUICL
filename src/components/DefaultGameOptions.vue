<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import {
    COMMON_OPTION_IDS,
    uniqueGameOptions,
    supportedGameOption,
    type DefaultGameOptions,
    type GameOptionDef,
    type GameOptionValue
} from '@shared/gameOptions';
import { getDefaultGameOptions, setDefaultGameOptions, errText } from '../api';
import { store, toast } from '../store';
const emit = defineEmits<{ section: [value: 'keys' | 'packs'] }>();
const state = ref<DefaultGameOptions>({ enabled: false, values: {} });
const draft = ref<Record<string, GameOptionValue>>({});
const busy = ref(false),
    loading = ref(true),
    version = ref('26.2');
const versions = computed(() => [
    ...new Set([
        '26.2',
        ...store.installed.map((v) => v.mcVersion).filter(Boolean),
        '1.21.11',
        '1.20.1',
        '1.16.5',
        '1.12.2'
    ])
]);
/** 常用一级页：固定顺序，从全部定义里取 */
const commonRows = computed(() =>
    COMMON_OPTION_IDS.map((id) => uniqueGameOptions.find((d) => d.id === id)!).filter(Boolean)
);
const unavailable = computed(() =>
    uniqueGameOptions.filter(
        (d) => d.id in state.value.values && !supportedGameOption(d, version.value)
    )
);
const value = (d: GameOptionDef) => draft.value[d.id] ?? state.value.values[d.id] ?? d.initial;
const label = (d: GameOptionDef) =>
    d.choices?.find((c) => c[0] === value(d))?.[1] ??
    (typeof value(d) === 'boolean'
        ? value(d)
            ? '开启'
            : '关闭'
        : String(value(d)) + (d.unit ?? ''));
async function save(change: Parameters<typeof setDefaultGameOptions>[0]) {
    if (busy.value) return;
    busy.value = true;
    try {
        state.value = await setDefaultGameOptions(change);
        draft.value = {};
    } catch (e) {
        draft.value = {};
        toast('保存游戏选项失败：' + errText(e), 'error');
    } finally {
        busy.value = false;
    }
}
function cycle(d: GameOptionDef) {
    const choices = d.choices?.map((c) => c[0]) ?? [false, true];
    void save({ id: d.id, value: choices[(choices.indexOf(value(d)) + 1) % choices.length] });
}
function numericChange(d: GameOptionDef, event: Event) {
    const input = event.target as HTMLInputElement;
    if (!input.checkValidity() || input.value === '') {
        input.value = String(value(d));
        return;
    }
    void save({ id: d.id, value: Number(input.value) });
}
onMounted(async () => {
    try {
        state.value = await getDefaultGameOptions();
    } catch (e) {
        toast(errText(e), 'error');
    } finally {
        loading.value = false;
    }
});
</script>

<template>
    <section
        data-ui="DefaultGameOptions:28b5a6c34e3b"
        class="card game-options"
        :data-design-page="'game-options-root'"
    >
        <header data-ui="DefaultGameOptions:6b4ccdfa01fc" class="options-header">
            <div data-ui="DefaultGameOptions:f45beec037f5">
                <h2 data-ui="DefaultGameOptions:8bae28f66b28">游戏选项</h2>
                <p class="muted">常用项直接调，下次启动时写入游戏；未修改项保留游戏原设置。</p>
            </div>
            <label data-ui="DefaultGameOptions:e3d3749c5eee" class="options-sync"
                ><span>启动时同步</span
                ><span data-ui="DefaultGameOptions:80969e0561bb" class="switch"
                    ><input
                        data-ui="DefaultGameOptions:c814db8dde4e"
                        type="checkbox"
                        :checked="state.enabled"
                        :disabled="busy || loading"
                        @change="
                            save({ enabled: ($event.target as HTMLInputElement).checked })
                        " /><span
                        data-ui="DefaultGameOptions:63f93b27a5fb"
                        class="switch-ui"
                    ></span></span
            ></label>
        </header>
        <div data-ui="DefaultGameOptions:66d723e1d987" class="options-context">
            <span data-ui="DefaultGameOptions:9de7c18725e7" class="tag"
                >{{ Object.keys(state.values).length }} 项自定义</span
            >
            <label data-ui="DefaultGameOptions:0e5b899edc75"
                >兼容性预览
                <select data-ui="DefaultGameOptions:9b4bafc1fc30" v-model="version" class="input">
                    <option v-for="v in versions" :key="v" :value="v">Minecraft {{ v }}</option>
                </select></label
            >
        </div>
        <p class="options-note muted">
            只同步你修改的项目，其余保留游戏设置。预览版本用于检查兼容性，启动时会按实际版本转换数值。
        </p>
        <p
            data-ui="DefaultGameOptions:10d78a0fdb4e"
            v-if="unavailable.length"
            class="options-warning"
        >
            {{ version }} 不支持：{{
                unavailable.map((d) => d.label).join('、')
            }}。这些项目仅对支持它们的版本生效。
        </p>
        <div data-ui="DefaultGameOptions:389b53ed107f" v-if="loading" class="empty">
            正在读取配置…
        </div>
        <!-- 常用参数一级可见：玩家最常调的项直接铺开 -->
        <div
            data-ui="DefaultGameOptions:0a11c0ffee001"
            v-else
            class="mc-options-grid"
        >
                    <div
                        data-ui="DefaultGameOptions:0a11c0ffee002"
                        v-for="d in commonRows"
                        :key="'common-' + d.id"
                        class="option-cell"
                        :class="{
                            'option-custom': d.id in state.values,
                            'option-unsupported': !supportedGameOption(d, version)
                        }"
                    >
                        <div class="option-caption">
                            <span>{{ d.label }}</span
                            ><label
                                data-ui="DefaultGameOptions:0a11c0ffee003"
                                v-if="typeof d.initial === 'number' && !d.choices"
                                class="option-number"
                                ><input
                                    data-ui="DefaultGameOptions:0a11c0ffee004"
                                    type="number"
                                    :aria-label="d.label + '数值'"
                                    :min="d.min"
                                    :max="d.max"
                                    :step="d.step"
                                    :value="value(d)"
                                    :disabled="busy || !supportedGameOption(d, version)"
                                    @change="numericChange(d, $event)"
                                /><span>{{ d.unit }}</span></label
                            ><strong data-ui="DefaultGameOptions:0a11c0ffee005" v-else>{{
                                label(d)
                            }}</strong>
                        </div>
                        <input
                            data-ui="DefaultGameOptions:0a11c0ffee006"
                            v-if="typeof d.initial === 'number' && !d.choices"
                            type="range"
                            :aria-label="d.label"
                            :min="d.min"
                            :max="d.max"
                            :step="d.step"
                            :value="value(d)"
                            :disabled="busy || !supportedGameOption(d, version)"
                            @input="draft[d.id] = Number(($event.target as HTMLInputElement).value)"
                            @change="
                                save({
                                    id: d.id,
                                    value: Number(($event.target as HTMLInputElement).value)
                                })
                            "
                        />
                        <select
                            data-ui="DefaultGameOptions:0a11c0ffee007"
                            v-else-if="d.choices && d.choices.length > 3"
                            class="input option-select"
                            :aria-label="d.label"
                            :value="value(d)"
                            :disabled="busy || !supportedGameOption(d, version)"
                            @change="
                                save({
                                    id: d.id,
                                    value: d.choices.find(
                                        (c) =>
                                            String(c[0]) ===
                                            ($event.target as HTMLSelectElement).value
                                    )![0]
                                })
                            "
                        >
                            <option
                                v-for="[v, name] in d.choices"
                                :key="String(v)"
                                :value="String(v)"
                            >
                                {{ name }}
                            </option>
                        </select>
                        <button
                            data-ui="DefaultGameOptions:0a11c0ffee008"
                            v-else
                            class="option-toggle"
                            :aria-label="d.label"
                            :disabled="busy || !supportedGameOption(d, version)"
                            @click="cycle(d)"
                        >
                            {{ label(d) }} <span>↻</span>
                        </button>
                        <div data-ui="DefaultGameOptions:0a11c0ffee009" class="option-foot">
                            <small data-ui="DefaultGameOptions:0a11c0ffee00a">{{
                                !supportedGameOption(d, version)
                                    ? d.until
                                        ? '此版本已移除此选项'
                                        : '需要 Minecraft ' + d.since + ' 或更新版本'
                                    : d.id in state.values
                                      ? state.enabled
                                          ? '将应用到游戏'
                                          : '已保存 · 同步未开启'
                                      : '跟随游戏 · 尚未覆盖'
                            }}</small
                            ><button
                                data-ui="DefaultGameOptions:0a11c0ffee00b"
                                v-if="d.id in state.values"
                                :disabled="busy"
                                @click="save({ id: d.id, value: null })"
                            >
                                跟随游戏</button
                            ><button
                                data-ui="DefaultGameOptions:0a11c0ffee00c"
                                v-else
                                :disabled="busy || !supportedGameOption(d, version)"
                                @click="save({ id: d.id, value: value(d) })"
                            >
                                应用此值
                            </button>
                        </div>
                    </div>
                </div>
                <!-- 全部选项（高级折叠）：完整的原版 89 项 -->
                <details data-ui="DefaultGameOptions:0a11c0ffee010" class="options-advanced">
                    <summary>全部选项（{{ uniqueGameOptions.length }} 项）…</summary>
                    <div class="mc-options-grid">
                        <div
                            data-ui="DefaultGameOptions:07ce737f0b81"
                            v-for="d in uniqueGameOptions"
                            :key="'adv-' + d.id"
                            class="option-cell"
                            :class="{
                                'option-custom': d.id in state.values,
                                'option-unsupported': !supportedGameOption(d, version)
                            }"
                        >
                            <div class="option-caption">
                                <span>{{ d.label || d.id }}</span
                                ><label
                                    data-ui="DefaultGameOptions:b2052d5c2d85"
                                    v-if="typeof d.initial === 'number' && !d.choices"
                                    class="option-number"
                                    ><input
                                        data-ui="DefaultGameOptions:77a5c1dbe109"
                                        type="number"
                                        :aria-label="d.label + '数值'"
                                        :min="d.min"
                                        :max="d.max"
                                        :step="d.step"
                                        :value="value(d)"
                                        :disabled="busy || !supportedGameOption(d, version)"
                                        @change="numericChange(d, $event)"
                                    /><span>{{ d.unit }}</span></label
                                ><strong data-ui="DefaultGameOptions:c39dcd5dabdd" v-else>{{
                                    label(d)
                                }}</strong>
                            </div>
                            <input
                                data-ui="DefaultGameOptions:0d2326a37a73"
                                v-if="typeof d.initial === 'number' && !d.choices"
                                type="range"
                                :aria-label="d.label"
                                :min="d.min"
                                :max="d.max"
                                :step="d.step"
                                :value="value(d)"
                                :disabled="busy || !supportedGameOption(d, version)"
                                @input="
                                    draft[d.id] = Number(($event.target as HTMLInputElement).value)
                                "
                                @change="
                                    save({
                                        id: d.id,
                                        value: Number(($event.target as HTMLInputElement).value)
                                    })
                                "
                            />
                            <select
                                data-ui="DefaultGameOptions:573cbf8cab33"
                                v-else-if="d.choices && d.choices.length > 3"
                                class="input option-select"
                                :aria-label="d.label"
                                :value="value(d)"
                                :disabled="busy || !supportedGameOption(d, version)"
                                @change="
                                    save({
                                        id: d.id,
                                        value: d.choices.find(
                                            (c) =>
                                                String(c[0]) ===
                                                ($event.target as HTMLSelectElement).value
                                        )![0]
                                    })
                                "
                            >
                                <option
                                    v-for="[v, name] in d.choices"
                                    :key="String(v)"
                                    :value="String(v)"
                                >
                                    {{ name }}
                                </option>
                            </select>
                            <button
                                data-ui="DefaultGameOptions:085c7f6ac800"
                                v-else
                                class="option-toggle"
                                :aria-label="d.label"
                                :disabled="busy || !supportedGameOption(d, version)"
                                @click="cycle(d)"
                            >
                                {{ label(d) }} <span>↻</span>
                            </button>
                            <div data-ui="DefaultGameOptions:1688f6a7f3fd" class="option-foot">
                                <small data-ui="DefaultGameOptions:38e2caddc1ea">{{
                                    !supportedGameOption(d, version)
                                        ? d.until
                                            ? '此版本已移除此选项'
                                            : '需要 Minecraft ' + d.since + ' 或更新版本'
                                        : d.id in state.values
                                          ? state.enabled
                                              ? '将应用到游戏'
                                              : '已保存 · 同步未开启'
                                          : '跟随游戏 · 尚未覆盖'
                                }}</small
                                ><button
                                    data-ui="DefaultGameOptions:505c33332d96"
                                    v-if="d.id in state.values"
                                    :disabled="busy"
                                    @click="save({ id: d.id, value: null })"
                                >
                                    跟随游戏</button
                                ><button
                                    data-ui="DefaultGameOptions:1de07779d63b"
                                    v-else
                                    :disabled="busy || !supportedGameOption(d, version)"
                                    @click="save({ id: d.id, value: value(d) })"
                                >
                                    应用此值
                                </button>
                            </div>
                        </div>
                    </div>
                </details>
        <details class="options-note muted">
            <summary>同步规则与兼容性说明</summary>
            <p>
                只同步已修改的原版选项，未修改项跟随游戏。预览版本用于检查兼容性；启动时按实例真实版本转换数值。模组接管的选项需同时检查模组配置。
            </p>
        </details>
    </section>
</template>

<style scoped>
.options-advanced {
    margin-top: var(--space-4);
}
.options-advanced summary {
    cursor: pointer;
    color: var(--accent-2);
    font-size: 13px;
    user-select: none;
}
.options-advanced .mc-options-grid {
    margin-top: var(--space-3);
}
.option-number {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--accent-2);
}
.option-number input {
    width: 76px;
    text-align: right;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--card);
    color: var(--text);
    padding: 3px 5px;
    font: inherit;
    font-variant-numeric: tabular-nums;
}
.option-select {
    width: 100%;
}
.game-options {
    padding: 28px;
}
.options-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 20px;
}
.options-header h2 {
    font-size: 20px;
    margin: 0 0 7px;
}
.options-header p {
    margin: 0;
    font-size: 13px;
}
.options-sync {
    display: flex;
    align-items: center;
    gap: 10px;
    white-space: nowrap;
    font-size: 13px;
}
.options-context {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    margin-top: 24px;
    padding-top: 20px;
    border-top: 1px solid var(--border);
}
.options-context label {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-dim);
    font-size: 12px;
}
.options-context select {
    width: 180px;
}
.options-note {
    font-size: 12px;
    line-height: 1.7;
    margin: 14px 0;
}
.options-warning {
    padding: 12px;
    border-radius: 10px;
    background: var(--accent-soft);
    color: var(--text);
    font-size: 13px;
}
.mc-options-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
}
.option-cell {
    padding: 16px;
    background: var(--card-2);
    border: 1px solid var(--border);
    border-radius: 12px;
    min-width: 0;
}
.option-custom {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
}
.option-unsupported {
    opacity: 0.65;
}
.option-caption {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 13px;
    margin-bottom: 14px;
}
.option-caption strong {
    color: var(--accent-2);
    font-variant-numeric: tabular-nums;
}
.option-cell input[type='range'] {
    width: 100%;
    accent-color: var(--accent);
    margin: 0;
    height: 22px;
}
.option-toggle {
    width: 100%;
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 9px 12px;
    background: var(--card);
    color: var(--text);
    display: flex;
    justify-content: space-between;
    cursor: pointer;
}
.option-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-top: 12px;
    color: var(--text-dim);
}
.option-foot small {
    font-size: 11px;
}
.option-foot button {
    border: 0;
    background: none;
    color: var(--accent-2);
    font-size: 11px;
    cursor: pointer;
    white-space: nowrap;
}
@media (max-width: 850px) {
    .options-header,
    .options-context {
        flex-wrap: wrap;
    }
    .game-options {
        padding: 20px;
    }
}
@media (max-width: 650px) {
    .mc-options-grid {
        grid-template-columns: 1fr;
    }
    .options-context label {
        flex-wrap: wrap;
    }
}
.game-options {
    padding: 20px;
}
.options-header {
    align-items: center;
}
.options-context {
    margin-top: 12px;
    padding-top: 12px;
    gap: 12px;
    flex-wrap: wrap;
}
.option-cell {
    border: 0;
    border-radius: 0;
    background: transparent;
    padding: 12px 0;
    border-bottom: 1px solid var(--border);
}
.options-note summary {
    cursor: pointer;
}
.option-caption {
    font-size: 14px;
}
.option-foot small,
.option-foot button {
    font-size: 12px;
}
</style>
