/**
 * WUHUICL 实验室兼容层：@modrinth/ui 的 composable 子集。
 * i18n 直接返回 defaultMessage（移植时已把 Axolotl zh-CN 语言包文案填入 defaultMessage），
 * 通知走 store.toast。
 */
import { ref } from 'vue';
import { errText, } from '../../api';
import { toast } from '../../store';

export type MessageDescriptor = { id: string; defaultMessage: string };

/** FormatJS defineMessages 的直通实现（仅保留类型信息） */
export function defineMessages<T extends Record<string, MessageDescriptor>>(messages: T): T {
    return messages;
}

/** {name} 占位符替换（ICU simple argument 子集，两个工具只用到这一档） */
function interpolate(template: string, values?: Record<string, string | number>): string {
    if (!values) return template;
    return template.replace(/\{(\w+)\}/g, (raw, key: string) =>
        Object.prototype.hasOwnProperty.call(values, key) ? String(values[key]) : raw
    );
}

export function useVIntl() {
    // WUHUICL 固定中文；locale 以 ref 形式给出（页面用于 toLocaleLowerCase）
    const locale = ref('zh-CN');
    return {
        locale,
        formatMessage: (message: MessageDescriptor, values?: Record<string, string | number>) =>
            interpolate(message?.defaultMessage ?? '', values)
    };
}

export type Notification = {
    type?: 'success' | 'error' | 'warning' | 'info';
    title: string;
    content?: string;
};

export function injectNotificationManager() {
    return {
        addNotification(notification: Notification) {
            toast(
                notification.content ? `${notification.title}：${notification.content}` : notification.title,
                notification.type === 'error' ? 'error' : 'success'
            );
        },
        handleError(error: unknown) {
            toast(errText(error), 'error');
        }
    };
}

/** 中文相对时间（对应 @modrinth/ui useRelativeTime） */
export function useRelativeTime() {
    const rtf = new Intl.RelativeTimeFormat('zh-CN', { numeric: 'auto' });
    return (input: number | string | Date): string => {
        const date = input instanceof Date ? input : new Date(input);
        const seconds = (date.getTime() - Date.now()) / 1000;
        const abs = Math.abs(seconds);
        if (abs < 10) return '刚刚';
        if (abs < 60) return rtf.format(Math.round(seconds), 'second');
        if (abs < 3600) return rtf.format(Math.round(seconds / 60), 'minute');
        if (abs < 86400) return rtf.format(Math.round(seconds / 3600), 'hour');
        if (abs < 86400 * 30) return rtf.format(Math.round(seconds / 86400), 'day');
        return date.toLocaleDateString();
    };
}

/** 字节数格式化（对应 @modrinth/ui useFormatBytes） */
export function useFormatBytes() {
    return (bytes: number): string => {
        if (!Number.isFinite(bytes) || bytes < 0) return '';
        if (bytes < 1024) return `${bytes} B`;
        const units = ['KiB', 'MiB', 'GiB', 'TiB'];
        let value = bytes;
        let unit = 'B';
        for (const next of units) {
            if (value < 1024) break;
            value /= 1024;
            unit = next;
        }
        return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${unit}`;
    };
}
