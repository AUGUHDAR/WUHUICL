<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
/** 全量清单项：type 供彩色角标（PCL 式——正式版/快照/旧版一眼可辨） */
type VersionType = 'release' | 'snapshot' | 'old_beta' | 'old_alpha';
const props = defineProps<{
    modelValue: string;
    versions: Array<{ id: string; type: VersionType }>;
    loading?: boolean;
}>();
const emit = defineEmits<{ 'update:modelValue': [value: string]; change: [] }>();
const input = ref<HTMLInputElement>(),
    menu = ref<HTMLElement>(),
    open = ref(false),
    active = ref(0),
    keyboardSelection = ref(false);
const text = ref(props.modelValue);
const options = computed(() => [
    { id: '', type: 'release' as VersionType },
    ...props.versions
        .filter((v) => v.id.toLowerCase().includes(text.value.trim().toLowerCase()))
        .slice(0, 200)
]);
const typeText: Record<VersionType, string> = {
    release: '正式版',
    snapshot: '快照',
    old_beta: 'Beta',
    old_alpha: 'Alpha'
};
const position = ref<Record<string, string>>({});
function close() {
    open.value = false;
    removeEventListener('pointerdown', outside, true);
    removeEventListener('scroll', onScroll, true);
    removeEventListener('resize', close);
}
function show() {
    const rect = input.value?.getBoundingClientRect();
    if (!rect) return;
    const below = innerHeight - rect.bottom - 12,
        above = rect.top - 12;
    const up = below < 180 && above > below;
    position.value = {
        left: Math.max(8, Math.min(rect.left, innerWidth - rect.width - 8)) + 'px',
        width: Math.min(rect.width, innerWidth - 16) + 'px',
        maxHeight: Math.min(280, Math.max(48, up ? above : below)) + 'px',
        ...(up ? { bottom: innerHeight - rect.top + 4 + 'px' } : { top: rect.bottom + 4 + 'px' })
    };
    active.value = Math.max(0, options.value.findIndex((o) => o.id === props.modelValue));
    open.value = true;
    addEventListener('pointerdown', outside, true);
    addEventListener('scroll', onScroll, true);
    addEventListener('resize', close);
}
function outside(event: PointerEvent) {
    if (!input.value?.contains(event.target as Node) && !menu.value?.contains(event.target as Node))
        close();
}
function onScroll(event: Event) {
    if (!menu.value?.contains(event.target as Node)) close();
}
function choose(value: string) {
    text.value = value;
    emit('update:modelValue', value);
    emit('change');
    close();
    input.value?.focus({ preventScroll: true });
    close();
}
function typing() {
    keyboardSelection.value = false;
    show();
}
function keyboard(event: KeyboardEvent) {
    if (event.key === 'Escape' && open.value) {
        event.preventDefault();
        event.stopPropagation();
        text.value = props.modelValue;
        close();
        return;
    }
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault();
        keyboardSelection.value = true;
        if (!open.value) show();
        else
            active.value =
                event.key === 'Home'
                    ? 0
                    : event.key === 'End'
                      ? options.value.length - 1
                      : Math.max(
                            0,
                            Math.min(
                                options.value.length - 1,
                                active.value + (event.key === 'ArrowDown' ? 1 : -1)
                            )
                        );
        void nextTick(() =>
            menu.value
                ?.querySelector<HTMLElement>('[data-focused="true"]')
                ?.scrollIntoView({ block: 'nearest' })
        );
    } else if (event.key === 'Enter') {
        event.preventDefault();
        choose(
            open.value && keyboardSelection.value
                ? (options.value[active.value]?.id ?? text.value.trim())
                : text.value.trim()
        );
    } else if (event.key === 'Tab') {
        if (text.value.trim() !== props.modelValue) {
            emit('update:modelValue', text.value.trim());
            emit('change');
        }
        close();
    }
}
// An instance filter or reset can change the value while the text field is mounted.
watch(
    () => props.modelValue,
    (value) => {
        text.value = value;
    }
);
onBeforeUnmount(close);
</script>
<template>
    <div class="community-version-filter">
        <input
            ref="input"
            data-ui="CommunityView:df846b92dee0"
            v-model="text"
            class="input"
            aria-label="Minecraft 版本"
            role="combobox"
            aria-autocomplete="list"
            :aria-expanded="open"
            aria-controls="community-version-options"
            :aria-activedescendant="
                open && keyboardSelection ? 'community-version-option-' + active : undefined
            "
            :placeholder="loading ? '加载版本列表…' : '全部版本'"
            @focus="
                keyboardSelection = false;
                show();
            "
            @click="
                keyboardSelection = false;
                show();
            "
            @input="typing"
            @keydown="keyboard"
            @blur="close"
        />
        <Teleport to="body">
            <div
                v-if="open"
                ref="menu"
                id="community-version-options"
                data-ui="CommunityView:8e1248a50470"
                class="community-version-popup"
                :style="position"
                role="listbox"
                @wheel.stop
            >
                <button
                    v-for="(version, i) in options"
                    :id="'community-version-option-' + i"
                    :key="version.id"
                    type="button"
                    role="option"
                    :aria-selected="modelValue === version.id"
                    :data-focused="active === i"
                    :class="{ selected: modelValue === version.id }"
                    @pointerdown.prevent
                    @click="choose(version.id)"
                >
                    <span class="cvf-name">{{ version.id || '全部版本' }}</span
                    ><span
                        v-if="version.id"
                        class="cvf-type"
                        :class="'is-' + version.type"
                        >{{ typeText[version.type] }}</span
                    >
                </button>
                <p v-if="options.length === 1 && text.trim()" class="muted">
                    没有匹配项；按 Enter 使用输入版本
                </p>
            </div>
        </Teleport>
    </div>
</template>
<style scoped>
.community-version-filter {
    flex: 1.4;
    min-width: 170px;
}
.community-version-filter .input {
    width: 100%;
}
.community-version-popup {
    position: fixed;
    z-index: 11000;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--card-solid, var(--card));
    box-shadow: var(--shadow);
}
.community-version-popup button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: var(--row-h);
    padding: 8px 12px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text);
    text-align: left;
    cursor: pointer;
    font: inherit;
    font-size: var(--text-sm);
}
.community-version-popup button:hover,
.community-version-popup button[data-focused='true'] {
    background: var(--hover);
}
.community-version-popup button.selected {
    background: var(--accent-soft);
    color: var(--text);
    font-weight: 600;
}
/* 类型彩色角标（PCL 式）：正式版=绿 / 快照=橙 / 旧版=灰 */
.cvf-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
}
.cvf-type {
    flex: none;
    font-size: 11px;
    line-height: 1.5;
    padding: 0 7px;
    border-radius: 999px;
    border: 1px solid transparent;
}
.cvf-type.is-release {
    color: #2e7d32;
    background: color-mix(in srgb, #4caf50 16%, transparent);
    border-color: color-mix(in srgb, #4caf50 35%, transparent);
}
.cvf-type.is-snapshot {
    color: #b26a00;
    background: color-mix(in srgb, #ff9800 16%, transparent);
    border-color: color-mix(in srgb, #ff9800 35%, transparent);
}
.cvf-type.is-old_beta,
.cvf-type.is-old_alpha {
    color: var(--text-dim);
    background: color-mix(in srgb, var(--text) 7%, transparent);
    border-color: color-mix(in srgb, var(--text) 16%, transparent);
}
.community-version-popup p {
    padding: 8px;
    font-size: var(--text-xs);
}
</style>
