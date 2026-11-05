<script setup lang="ts">
/**
 * 启动打卡热力图（首页卡片 + 详情弹窗）。
 * 每次游戏成功启动 +1（Rust 端 launch-stats.json）；颜色越深 = 当天启动次数越多。
 * 点按天格查看当天详情；点卡片打开完整年历（GitHub 风格但直读数字，不做无文字的悬停猜谜）。
 */
import { computed, onMounted, ref } from 'vue';
import { getLaunchHeatmap } from '../api';

type DayCell = {
    date: string;
    count: number;
    /** 供 CSS 用的强度档（0-4） */
    level: number;
    inFuture: boolean;
};

const days = ref<Record<string, number>>({});
const loading = ref(true);
const selected = ref<string>('');
const modalOpen = ref(false);
/** 弹窗年历向前翻页（0 = 本周收尾的最近 53 周） */
const modalOffsetWeeks = ref(0);

async function refresh() {
    try {
        days.value = (await getLaunchHeatmap()) ?? {};
    } catch {
        days.value = {};
    } finally {
        loading.value = false;
    }
}
onMounted(refresh);

function toKey(date: Date): string {
    const y = date.getFullYear();
    const m = String(date.getMonth() + 1).padStart(2, '0');
    const d = String(date.getDate()).padStart(2, '0');
    return `${y}-${m}-${d}`;
}

function levelOf(count: number): number {
    if (count <= 0) return 0;
    if (count === 1) return 1;
    if (count <= 3) return 2;
    if (count <= 6) return 3;
    return 4;
}

const todayKey = computed(() => toKey(new Date()));

/** 以今天收尾的周对齐日历：列 = 周（周日开头），行 = 周日..周六 */
function buildCalendar(weeks: number, offsetWeeks: number): DayCell[][] {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    // 本周的周六（日历列以完整周收尾）
    const weekEnd = new Date(today);
    weekEnd.setDate(weekEnd.getDate() + (6 - today.getDay()));
    const start = new Date(weekEnd);
    start.setDate(start.getDate() - (weeks - 1) * 7 - offsetWeeks * 7);
    const columns: DayCell[][] = [];
    const cursor = new Date(start);
    for (let week = 0; week < weeks; week++) {
        const column: DayCell[] = [];
        for (let day = 0; day < 7; day++) {
            const key = toKey(cursor);
            column.push({
                date: key,
                count: days.value[key] ?? 0,
                level: levelOf(days.value[key] ?? 0),
                inFuture: cursor > today,
            });
            cursor.setDate(cursor.getDate() + 1);
        }
        columns.push(column);
    }
    return columns;
}

const cardWeeks = computed(() => buildCalendar(16, 0));
const modalWeeks = computed(() => buildCalendar(53, modalOffsetWeeks.value));

/** 月标签：月第一格的列下标 → 月份名 */
function monthLabels(calendar: DayCell[][]): Array<{ index: number; label: string }> {
    const labels: Array<{ index: number; label: string }> = [];
    let lastMonth = -1;
    calendar.forEach((column, index) => {
        const month = Number(column[0].date.slice(5, 7));
        if (month !== lastMonth) {
            labels.push({ index, label: `${month}月` });
            lastMonth = month;
        }
    });
    return labels;
}
const cardMonths = computed(() => monthLabels(cardWeeks.value));
const modalMonths = computed(() => monthLabels(modalWeeks.value));

const totalLaunches = computed(() =>
    Object.values(days.value).reduce((sum, count) => sum + count, 0)
);
const todayCount = computed(() => days.value[todayKey.value] ?? 0);

/** 连续打卡天数（从今天/昨天向回数有启动的日子） */
const streakDays = computed(() => {
    let streak = 0;
    const cursor = new Date();
    if (!(days.value[toKey(cursor)] > 0)) cursor.setDate(cursor.getDate() - 1);
    for (;;) {
        if ((days.value[toKey(cursor)] ?? 0) > 0) {
            streak += 1;
            cursor.setDate(cursor.getDate() - 1);
        } else break;
    }
    return streak;
});
const activeDays = computed(() =>
    Object.values(days.value).filter((count) => count > 0).length
);

function formatDay(key: string): string {
    const [, month, day] = key.split('-');
    return `${Number(month)}月${Number(day)}日`;
}
const selectedLabel = computed(() => {
    if (!selected.value) return '';
    const count = days.value[selected.value] ?? 0;
    const suffix = selected.value === todayKey.value ? '（今天）' : '';
    return count > 0
        ? `${formatDay(selected.value)}${suffix} · 启动 ${count} 次`
        : `${formatDay(selected.value)}${suffix} · 当天没有启动记录`;
});

function pick(cell: DayCell) {
    if (cell.inFuture) return;
    selected.value = selected.value === cell.date ? '' : cell.date;
}
function shiftModal(direction: number) {
    modalOffsetWeeks.value = Math.max(0, modalOffsetWeeks.value + direction);
}
</script>

<template>
    <section class="heatmap-card card">
        <header class="heatmap-head">
            <h3>启动打卡</h3>
            <button type="button" class="btn btn-ghost btn-sm" @click="modalOpen = true">
                查看历史
            </button>
        </header>
        <p class="heatmap-summary">
            今天已启动 <strong>{{ todayCount }}</strong> 次 · 连续打卡
            <strong>{{ streakDays }}</strong> 天
        </p>
        <button
            type="button"
            class="heatmap-grid-button"
            aria-label="查看启动历史"
            @click="modalOpen = true"
        >
            <span class="heatmap-months">
                <span
                    v-for="month in cardMonths"
                    :key="month.label + month.index"
                    class="heatmap-month"
                    :style="{ gridColumn: `${month.index + 1} / span 1` }"
                    >{{ month.label }}</span
                >
            </span>
            <span class="heatmap-grid" role="img" aria-label="启动热力图">
                <template v-for="(column, weekIndex) in cardWeeks" :key="weekIndex">
                    <span
                        v-for="cell in column"
                        :key="cell.date"
                        class="heatmap-cell"
                        :class="[`level-${cell.level}`, { future: cell.inFuture }]"
                    ></span>
                </template>
            </span>
        </button>
        <p v-if="selectedLabel" class="heatmap-detail">{{ selectedLabel }}</p>

        <Teleport to="body">
            <div v-if="modalOpen" class="modal-mask" @pointerdown.self="modalOpen = false">
                <div class="modal heatmap-modal">
                    <header class="heatmap-modal-head">
                        <h3 class="modal-title">启动历史</h3>
                        <div class="heatmap-modal-nav">
                            <button
                                type="button"
                                class="btn btn-ghost btn-sm"
                                :disabled="modalOffsetWeeks === 0"
                                @click="shiftModal(26)"
                            >
                                ← 更早
                            </button>
                            <button
                                type="button"
                                class="btn btn-ghost btn-sm"
                                :disabled="modalOffsetWeeks === 0"
                                @click="shiftModal(-26)"
                            >
                                回到今天 →</button
                            ><button
                                type="button"
                                class="btn btn-ghost btn-sm"
                                @click="modalOpen = false"
                            >
                                关闭
                            </button>
                        </div>
                    </header>
                    <div class="heatmap-stats">
                        <span>累计启动 <strong>{{ totalLaunches }}</strong> 次</span>
                        <span>有启动的天数 <strong>{{ activeDays }}</strong></span>
                        <span>连续打卡 <strong>{{ streakDays }}</strong> 天</span>
                    </div>
                    <div class="heatmap-modal-scroll">
                        <span class="heatmap-months modal-months">
                            <span
                                v-for="month in modalMonths"
                                :key="month.label + month.index"
                                class="heatmap-month"
                                :style="{ gridColumn: `${month.index + 1} / span 1` }"
                                >{{ month.label }}</span
                            >
                        </span>
                        <span class="heatmap-grid modal-grid">
                            <template v-for="(column, weekIndex) in modalWeeks" :key="weekIndex">
                                <button
                                    v-for="cell in column"
                                    :key="cell.date"
                                    type="button"
                                    class="heatmap-cell"
                                    :class="[
                                        `level-${cell.level}`,
                                        {
                                            future: cell.inFuture,
                                            selected: selected === cell.date
                                        }
                                    ]"
                                    :title="
                                        cell.inFuture
                                            ? ''
                                            : `${cell.date} · 启动 ${cell.count} 次`
                                    "
                                    :aria-label="`${cell.date} 启动 ${cell.count} 次`"
                                    @click="pick(cell)"
                                ></button>
                            </template>
                        </span>
                    </div>
                    <footer class="heatmap-legend">
                        <span>少</span>
                        <span
                            v-for="level in 5"
                            :key="level"
                            class="heatmap-cell"
                            :class="`level-${level - 1}`"
                        ></span>
                        <span>多</span>
                        <span class="heatmap-legend-note">颜色越深 = 当天启动次数越多</span>
                    </footer>
                    <p class="heatmap-detail">{{ selectedLabel || '点按任意日期查看当天的启动次数' }}</p>
                </div>
            </div>
        </Teleport>
    </section>
</template>

<style scoped>
.heatmap-card {
    display: grid;
    gap: 10px;
}
.heatmap-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
}
.heatmap-head h3 {
    margin: 0;
}
.heatmap-summary {
    margin: 0;
    font-size: 12.5px;
    color: var(--text-dim);
}
.heatmap-summary strong {
    color: var(--text);
}
.heatmap-grid-button {
    display: grid;
    gap: 4px;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: pointer;
    text-align: left;
}
.heatmap-months {
    display: grid;
    grid-template-columns: repeat(16, 1fr);
    font-size: 10.5px;
    color: var(--text-dim);
    line-height: 1;
}
.modal-months {
    grid-template-columns: repeat(53, 1fr);
}
.heatmap-month {
    overflow: visible;
    white-space: nowrap;
}
.heatmap-grid {
    display: grid;
    grid-template-rows: repeat(7, 1fr);
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    gap: 3px;
}
.modal-grid {
    gap: 3px;
}
.heatmap-cell {
    width: 100%;
    aspect-ratio: 1;
    border-radius: 3px;
    border: 0;
    padding: 0;
    background: color-mix(in srgb, var(--border) 42%, transparent);
}
.heatmap-cell.level-1 {
    background: color-mix(in srgb, var(--accent) 26%, transparent);
}
.heatmap-cell.level-2 {
    background: color-mix(in srgb, var(--accent) 48%, transparent);
}
.heatmap-cell.level-3 {
    background: color-mix(in srgb, var(--accent) 72%, transparent);
}
.heatmap-cell.level-4 {
    background: var(--accent-deep);
}
.heatmap-cell.future {
    background: transparent;
    pointer-events: none;
}
.heatmap-cell:not(.future):hover {
    outline: 1.5px solid var(--accent-2);
}
.heatmap-cell.selected {
    outline: 2px solid var(--accent);
}
.heatmap-detail {
    margin: 0;
    min-height: 1.2em;
    font-size: 12px;
    color: var(--text-dim);
}
.heatmap-modal {
    width: min(920px, calc(100vw - 48px));
}
.heatmap-modal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
}
.heatmap-modal-nav {
    display: flex;
    gap: 8px;
}
.heatmap-stats {
    display: flex;
    gap: 18px;
    flex-wrap: wrap;
    font-size: 13px;
    color: var(--text-dim);
}
.heatmap-stats strong {
    color: var(--text);
}
.heatmap-modal-scroll {
    overflow-x: auto;
    display: grid;
    gap: 4px;
    padding-bottom: 4px;
}
.heatmap-legend {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    color: var(--text-dim);
}
.heatmap-legend .heatmap-cell {
    width: 12px;
    height: 12px;
    flex: none;
}
.heatmap-legend-note {
    margin-left: 8px;
}
</style>
