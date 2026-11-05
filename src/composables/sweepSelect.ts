/**
 * PCL 式扫选（drag-select）：按住行/卡纵向拖动 → 沿途批量选中/反选。
 * 方向由锚点行原状态决定：未选中 → 扫选；已选中 → 反扫。
 * 横向占优的拖动不接管——各页 dragstart 手柄先调 interceptFileDrag 问路，
 * 纵向则取消文件拖出并就地开始扫选，横向仍走原有「拖出文件」。
 */
import { onUnmounted } from 'vue';

export interface SweepSelectOptions {
    /** 含全部行/卡的容器（列表卡片或工作区），扫选中加 .sweeping 禁文本选择 */
    container: () => HTMLElement | null;
    /** 行/卡选择器；行元素需带 :data-sweep-key="唯一键" */
    itemSelector: string;
    /** 是否允许扫选（busy/loading/编辑模式时 false） */
    enabled?: () => boolean;
    /** 读某键当前选中态 */
    isSelected: (key: string) => boolean;
    /** 批量写选中态（一次响应式更新） */
    setMany: (keys: string[], selected: boolean) => void;
}

export function useSweepSelect(opts: SweepSelectOptions) {
    let pressed = false;
    let sweeping = false;
    let justSwept = false;
    let anchorKey = '';
    let anchorIdx = -1;
    let lastIdx = -1;
    let startX = 0;
    let startY = 0;
    let lastX = 0;
    let lastY = 0;
    let items: HTMLElement[] = [];
    let mode: 'select' | 'deselect' = 'select';

    function collect(): HTMLElement[] {
        const c = opts.container();
        if (!c) return [];
        return [...c.querySelectorAll<HTMLElement>(opts.itemSelector)].filter(
            (el) => el.dataset.sweepKey
        );
    }

    function beginSweep(key: string): boolean {
        items = collect();
        anchorKey = key;
        anchorIdx = items.findIndex((el) => el.dataset.sweepKey === key);
        if (anchorIdx < 0) return false;
        sweeping = true;
        mode = opts.isSelected(anchorKey) ? 'deselect' : 'select';
        opts.container()?.classList.add('sweeping');
        applyRange(anchorIdx, anchorIdx);
        lastIdx = anchorIdx;
        return true;
    }

    function applyRange(a: number, b: number) {
        if (a < 0 || b < 0) return;
        const lo = Math.min(a, b);
        const hi = Math.max(a, b);
        const keys: string[] = [];
        for (let i = lo; i <= hi; i++) {
            const key = items[i]?.dataset.sweepKey;
            if (key && opts.isSelected(key) !== (mode === 'select')) keys.push(key);
        }
        if (keys.length) opts.setMany(keys, mode === 'select');
    }

    function stopSweep() {
        if (sweeping) {
            justSwept = true;
            opts.container()?.classList.remove('sweeping');
            // click 在 pointerup 之后触发，只吞扫选结束的这一下
            setTimeout(() => {
                justSwept = false;
            }, 0);
        }
        sweeping = false;
        pressed = false;
        anchorKey = '';
        anchorIdx = -1;
        lastIdx = -1;
    }

    function onPointerDown(e: PointerEvent) {
        if (e.button !== 0 || e.pointerType === 'touch') return;
        if (opts.enabled && !opts.enabled()) return;
        const item = (e.target as HTMLElement | null)?.closest?.(
            opts.itemSelector
        ) as HTMLElement | null;
        if (!item?.dataset.sweepKey) return;
        const c = opts.container();
        if (!c || !c.contains(item)) return;
        pressed = true;
        anchorKey = item.dataset.sweepKey;
        startX = lastX = e.clientX;
        startY = lastY = e.clientY;
        anchorIdx = -1;
        lastIdx = -1;
        sweeping = false;
    }

    function onPointerMove(e: PointerEvent) {
        if (!pressed) return;
        if (e.buttons === 0) {
            // 上一次按压可能被原生拖出循环吞掉了 pointerup
            stopSweep();
            return;
        }
        lastX = e.clientX;
        lastY = e.clientY;
        if (!sweeping) {
            const dx = Math.abs(e.clientX - startX);
            const dy = Math.abs(e.clientY - startY);
            if (dy < 6 || dy <= dx) return;
            if (!beginSweep(anchorKey)) {
                stopSweep();
                return;
            }
        }
        const hit = document.elementFromPoint(e.clientX, e.clientY)?.closest?.(
            opts.itemSelector
        ) as HTMLElement | null;
        const idx = hit ? items.indexOf(hit) : -1;
        if (idx >= 0 && idx !== lastIdx) {
            applyRange(anchorIdx, idx);
            lastIdx = idx;
        }
        const sel = window.getSelection();
        if (sel && sel.rangeCount) sel.removeAllRanges();
    }

    function onPointerUp() {
        stopSweep();
    }

    function onClickCapture(e: MouseEvent) {
        if (!justSwept) return;
        e.stopPropagation();
        e.preventDefault();
    }

    /** dragstart 手柄先问这一句：纵向拖 = 扫选接管（返回 true，调用方直接 return） */
    function interceptFileDrag(event: DragEvent): boolean {
        if (sweeping) {
            event.preventDefault();
            event.stopPropagation();
            return true;
        }
        if (!pressed) return false;
        const dy = Math.abs(lastY - startY);
        const dx = Math.abs(lastX - startX);
        if (dy > 4 && dy > dx) {
            event.preventDefault();
            event.stopPropagation();
            const item = (event.target as HTMLElement | null)?.closest?.(
                opts.itemSelector
            ) as HTMLElement | null;
            if (item?.dataset.sweepKey) beginSweep(item.dataset.sweepKey);
            return true;
        }
        return false;
    }

    window.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp);
    window.addEventListener('pointercancel', onPointerUp);
    window.addEventListener('click', onClickCapture, true);
    onUnmounted(() => {
        window.removeEventListener('pointerdown', onPointerDown, true);
        window.removeEventListener('pointermove', onPointerMove);
        window.removeEventListener('pointerup', onPointerUp);
        window.removeEventListener('pointercancel', onPointerUp);
        window.removeEventListener('click', onClickCapture, true);
    });

    return { interceptFileDrag };
}
