/** OverflowMenu 菜单项（独立文件：.vue SFC 无法被 tsc 直接 re-export 类型）。 */
import type { Component } from 'vue';

export type OverflowMenuOption = {
    id: string;
    label?: string;
    icon?: Component;
    color?: 'brand' | 'red' | 'highlight';
    disabled?: boolean;
    action?: () => void;
    divider?: boolean;
};
