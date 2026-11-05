/**
 * 实验室兼容层出口：把 @modrinth/ui 在两个工具（种子地图 / 投影工坊）里
 * 用到的组件与 composable 以同名导出，页面源码得以近乎原样移植。
 * 样式（ui.css）走 wuhuicl 主题变量；i18n 直出中文 defaultMessage。
 */
export { default as Button } from './Button.vue';
export { default as Checkbox } from './Checkbox.vue';
export { default as Combobox } from './Combobox.vue';
export { default as Toggle } from './Toggle.vue';
export { default as Slider } from './Slider.vue';
export { default as StyledInput } from './StyledInput.vue';
export { default as Admonition } from './Admonition.vue';
export { default as Accordion } from './Accordion.vue';
export { default as PopoutMenu } from './PopoutMenu.vue';
export { default as NewModal } from './NewModal.vue';
export { default as EmptyState } from './EmptyState.vue';
export { default as OverflowMenu } from './OverflowMenu.vue';
export { default as Tabs } from './Tabs.vue';
export { default as Avatar } from './Avatar.vue';
export { default as TagItem } from './TagItem.vue';
export type { OverflowMenuOption } from './overflow-menu-option';
export { vTooltip as tooltip } from './tooltip';
export {
    defineMessages,
    injectNotificationManager,
    useFormatBytes,
    useRelativeTime,
    useVIntl
} from './composables';
export type { MessageDescriptor, Notification } from './composables';
export { useVirtualScroll } from './virtual-scroll';
