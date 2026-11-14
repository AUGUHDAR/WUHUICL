<script setup lang="ts">
/**
 * 全站统一浮层菜单原语（UI_SYSTEM §5 弹层五类之一）。
 * 口径（M3/NN/g）：溢出菜单只放情境补充动作——短标签、分组呈现、轻量不打断；
 * 与既有页面重复的入口应收敛掉，而不是堆进菜单。
 *
 * 交互契约（UX「焦点与键盘」+ SelectMenu 同款纪律）：
 * - ↑↓/Home/End 在菜单项间移动（分组标题跳过），Enter 选中，ESC/Tab/点击外部关闭；
 * - 关闭后焦点归还触发元素；
 * - 视口夹持 + 上下翻转定位（底部空间不足时向上翻）；
 * - 置灰项必须给 disabledReason 悬浮说明（UX「联动禁用要显式」）。
 */
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from 'vue';

export interface AppMenuItem {
    id: string;
    label: string;
    /** 危险动作：红色文案与危险 hover（删除类放最后一组） */
    danger?: boolean;
    disabled?: boolean;
    /** 置灰原因与恢复方式（UX「联动禁用要显式」） */
    disabledReason?: string;
    /** 所属分组名；分组按 props.groups 顺序呈现，组间画分隔线（Fluent「organizing commands」） */
    group?: string;
}

const props = withDefaults(
    defineProps<{
        open: boolean;
        /** 触发元素：定位与翻转依据，关闭后焦点归还 */
        anchor: HTMLElement | null;
        items: AppMenuItem[];
        /** 分组标题按此顺序呈现；未列出的组按首次出现顺序排在后面 */
        groups?: string[];
        /** 触发方向 end = 菜单右缘对齐触发元素右缘（行尾 ⋯ 按钮用） */
        align?: 'start' | 'end';
        minWidth?: number;
        ariaLabel?: string;
    }>(),
    { align: 'start', minWidth: 210 }
);
const emit = defineEmits<{ (e: 'close'): void; (e: 'select', id: string): void }>();

const menuEl = ref<HTMLElement | null>(null);
const style = ref<Record<string, string>>({});
const active = ref(-1);
const menuId = useId();

interface Row {
    kind: 'label' | 'item' | 'sep';
    text?: string;
    item?: AppMenuItem;
    itemIndex?: number;
}

const rows = computed<Row[]>(() => {
    const byGroup = new Map<string, AppMenuItem[]>();
    for (const item of props.items) {
        const key = item.group ?? '';
        if (!byGroup.has(key)) byGroup.set(key, []);
        byGroup.get(key)!.push(item);
    }
    const groupNames = props.groups ?? [];
    const orderedKeys = [
        ...groupNames.filter((g) => byGroup.has(g)),
        ...[...byGroup.keys()].filter((g) => !groupNames.includes(g))
    ];
    const out: Row[] = [];
    let index = 0;
    orderedKeys.forEach((g, gi) => {
        if (gi > 0) out.push({ kind: 'sep' });
        if (g) out.push({ kind: 'label', text: g });
        for (const item of byGroup.get(g)!) {
            out.push({ kind: 'item', item, itemIndex: index++ });
        }
    });
    return out;
});

const itemCount = computed(() => rows.value.filter((r) => r.kind === 'item').length);

function close() {
    emit('close');
    removeEventListener('pointerdown', onPointerDown, true);
    removeEventListener('keydown', onKeydown, true);
    removeEventListener('resize', close);
    removeEventListener('scroll', onScroll, true);
}

function place() {
    const rect = props.anchor?.getBoundingClientRect();
    if (!rect) return;
    const width = Math.min(Math.max(props.minWidth, 220), innerWidth - 24);
    const desired = 360;
    // 锚点夹持进视口：页面自身滚出视口外时（极矮窗口），菜单仍贴边完整呈现而非跟到屏外
    const top = Math.max(0, Math.min(rect.top, innerHeight));
    const bottom = Math.max(0, Math.min(rect.bottom, innerHeight));
    const below = Math.max(0, innerHeight - bottom - 12);
    const above = Math.max(0, top - 12);
    const fit = Math.min(desired, Math.max(innerHeight - 24, 140));
    let left = props.align === 'end' ? rect.right - width : rect.left;
    left = Math.max(12, Math.min(left, innerWidth - width - 12));
    /* 翻转判定（用户实测 bug：底行卡片菜单朝下展开、捅出视口底部被截断）——
     * 旧式 `up = below < 200 && above > below` 配 `maxH = max(below, above)` 自相矛盾：
     * 判定向下的分支里 maxH 仍取上下空间较大者，菜单以 360px 全高渲染在下方空间里
     * 直接捅穿视口底（顶行同理：below < 360 时向下展开必然溢出）。
     * 修法：优先选能完整放下 fit 的一侧（below ≥ fit → 向下，否则 above 更大 → 向上），
     * 两侧都不足 fit 时选空间更大的一边、maxH 钳到该侧空间——菜单永不溢出，
     * 内容超限时菜单内部滚动（危险区项滚动可达；能放下 fit 的一侧完整无滚动）。 */
    const up = below >= fit ? false : above >= below;
    const maxH = Math.min(fit, up ? above : below);
    style.value = up
        ? { left: left + 'px', bottom: innerHeight - top + 6 + 'px', width: width + 'px', maxHeight: maxH + 'px' }
        : { left: left + 'px', top: bottom + 6 + 'px', width: width + 'px', maxHeight: maxH + 'px' };
}

function openMenu() {
    place();
    active.value = -1;
    addEventListener('pointerdown', onPointerDown, true);
    addEventListener('keydown', onKeydown, true);
    addEventListener('resize', close);
    addEventListener('scroll', onScroll, true);
}

function onPointerDown(e: PointerEvent) {
    if (!menuEl.value?.contains(e.target as Node) && !props.anchor?.contains(e.target as Node))
        close();
}
function onScroll(event: Event) {
    if (!menuEl.value?.contains(event.target as Node)) close();
}

function focusActive() {
    void nextTick(() =>
        menuEl.value
            ?.querySelector<HTMLElement>('[data-focused=true]')
            ?.scrollIntoView({ block: 'nearest' })
    );
}

function choose(item: AppMenuItem) {
    if (item.disabled) return;
    emit('select', item.id);
    close();
    props.anchor?.focus({ preventScroll: true });
}

function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Tab') {
        close();
        return;
    }
    if (e.key === 'Escape') {
        e.preventDefault();
        e.stopImmediatePropagation();
        close();
        props.anchor?.focus();
        return;
    }
    const step = (delta: number) => {
        active.value = Math.max(0, Math.min(itemCount.value - 1, active.value + delta));
        focusActive();
    };
    if (e.key === 'ArrowDown') {
        e.preventDefault();
        e.stopImmediatePropagation();
        active.value < 0 ? (active.value = 0) : step(1);
        focusActive();
    } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        e.stopImmediatePropagation();
        active.value < 0 ? (active.value = itemCount.value - 1) : step(-1);
        focusActive();
    } else if (e.key === 'Home') {
        e.preventDefault();
        e.stopImmediatePropagation();
        active.value = 0;
        focusActive();
    } else if (e.key === 'End') {
        e.preventDefault();
        e.stopImmediatePropagation();
        active.value = itemCount.value - 1;
        focusActive();
    } else if (e.key === 'Enter') {
        const row = rows.value.find((r) => r.kind === 'item' && r.itemIndex === active.value);
        if (row?.item) {
            e.preventDefault();
            e.stopImmediatePropagation();
            choose(row.item);
        }
    }
}

watch(
    () => props.open,
    (v) => {
        if (v) openMenu();
        else close();
    }
);
// 打开期间锚点/条目变化时重新定位
watch(
    () => props.items.length,
    () => {
        if (props.open) place();
    }
);
onBeforeUnmount(close);
</script>

<template>
    <Teleport to="body">
        <Transition name="popover">
            <div
                v-if="open"
                :id="menuId"
                ref="menuEl"
                class="float-menu app-menu"
                role="menu"
                :aria-label="ariaLabel"
                :style="style"
                tabindex="-1"
                @wheel.stop
            >
                <div v-if="$slots.header" class="app-menu-header">
                    <slot name="header" />
                </div>
                <template v-for="(row, i) in rows" :key="row.kind === 'item' ? row.item!.id : `g${i}`">
                    <div v-if="row.kind === 'sep'" class="menu-sep" role="separator"></div>
                    <span v-else-if="row.kind === 'label'" class="menu-group-label">{{ row.text }}</span>
                    <button
                        v-else
                        type="button"
                        role="menuitem"
                        class="menu-item"
                        :class="{ danger: row.item!.danger }"
                        :disabled="row.item!.disabled"
                        :data-focused="active === row.itemIndex"
                        :title="
                            row.item!.disabled
                                ? row.item!.disabledReason
                                : row.item!.label
                        "
                        @click="choose(row.item!)"
                    >
                        {{ row.item!.label }}
                    </button>
                </template>
            </div>
        </Transition>
    </Teleport>
</template>

<style scoped>
.app-menu {
    /* 位置由 place() 内联给出；这里只补足形态 */
    max-width: calc(100vw - 24px);
    outline: none;
}
.app-menu .menu-item[data-focused='true'] {
    background: var(--hover);
    color: var(--text);
}
.app-menu .menu-item.danger[data-focused='true'] {
    background: var(--danger-soft);
    color: var(--danger);
}
@media (prefers-reduced-motion: reduce) {
    .app-menu {
        transition: none;
        animation: none;
    }
}
</style>
