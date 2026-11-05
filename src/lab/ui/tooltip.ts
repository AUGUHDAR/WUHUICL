/**
 * v-tooltip 指令：@modrinth/ui 用 floating 提示，这里降级为原生 title
 * （两工具的 tooltip 全是短文案，原生提示已够用；支持 v-tooltip.top 写法，方位忽略）。
 */
import type { Directive } from 'vue';

type TooltipValue = string | { content?: string } | undefined | null;

function apply(el: HTMLElement, value: TooltipValue) {
    const text = typeof value === 'string' ? value : (value?.content ?? '');
    if (text) el.setAttribute('title', text);
    else el.removeAttribute('title');
}

export const vTooltip: Directive<HTMLElement, TooltipValue> = {
    mounted(el, binding) {
        apply(el, binding.value);
    },
    updated(el, binding) {
        apply(el, binding.value);
    },
    unmounted(el) {
        el.removeAttribute('title');
    }
};
