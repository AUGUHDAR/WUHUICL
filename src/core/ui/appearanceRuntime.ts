import { watch } from 'vue';
import type { CustomTheme, Settings, ThemeName } from '@shared/types';
import { DEFAULT_CUSTOM_THEME, THEME_PRESETS } from '@shared/types';
import { readableCustomColors } from '@shared/themeContrast';

/** 自定义主题写入的全部 inline CSS 变量（切回亮/暗时需统一清除） */
const CUSTOM_VARS = [
    '--accent',
    '--accent-2',
    '--accent-deep',
    '--accent-grad',
    '--accent-soft',
    '--on-accent',
    '--bg',
    '--bg-2',
    '--card',
    '--card-solid',
    '--card-2',
    '--text',
    '--text-dim',
    '--border',
    '--sidebar-text',
    '--bn-text',
    '--border-strong',
    '--scroll',
    '--hover',
    '--mask',
    '--danger',
    '--danger-soft',
    '--danger-border',
    '--ok',
    '--ok-soft',
    '--cyan',
    '--cyan-soft',
    '--shadow',
    '--shadow-lg',
    '--shell-surface',
    '--glass-blur',
    '--sidebar-w',
    '--banner-h',
    '--radius'
];

/** 解析 #rrggbb 并计算相对亮度（0-1），非法输入按暗色处理 */
function hexLuminance(hex: string): number {
    const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
    if (!m) return 0;
    const n = parseInt(m[1], 16);
    const r = (n >> 16) & 255;
    const g = (n >> 8) & 255;
    const b = n & 255;
    return (0.299 * r + 0.587 * g + 0.114 * b) / 255;
}

function clearCustomVars() {
    const st = document.documentElement.style;
    CUSTOM_VARS.forEach((v) => st.removeProperty(v));
}

/** 把 settings.custom 映射为 documentElement 上的 inline CSS 变量覆盖 */
function applyCustomVars(custom: CustomTheme, theme: ThemeName) {
    const st = document.documentElement.style;
    const colors = theme === 'custom' ? readableCustomColors(custom.colors) : custom.colors;
    const accent = colors.accent;
    const dark = hexLuminance(colors.bg) < 0.46;
    const cardOpacity = dark ? 88 : 94;
    const raisedOpacity = dark ? 82 : 91;
    const sideOpacity = dark ? 26 : 38;
    const accent2 = `color-mix(in srgb, ${accent} ${dark ? 72 : 84}%, ${dark ? 'white' : 'black'})`;
    const accentDeep = `color-mix(in srgb, ${accent} 78%, black)`;
    st.setProperty('--accent', accent);
    st.setProperty('--accent-2', accent2);
    st.setProperty('--accent-deep', accentDeep);
    st.setProperty(
        '--accent-grad',
        `linear-gradient(135deg, ${accent2} 0%, ${accent} 55%, ${accentDeep} 100%)`
    );
    st.setProperty('--accent-soft', `color-mix(in srgb, ${accent} 14%, transparent)`);
    st.setProperty('--on-accent', hexLuminance(accent) > 0.6 ? '#1a1208' : '#ffffff');
    st.setProperty('--bg', colors.bg);
    st.setProperty('--card', `color-mix(in srgb, ${colors.card} ${cardOpacity}%, transparent)`);
    st.setProperty('--card-solid', colors.card);
    st.setProperty('--card-2', `color-mix(in srgb, ${colors.card} ${raisedOpacity}%, transparent)`);
    st.setProperty('--text', colors.text);
    st.setProperty('--text-dim', colors.textDim);
    st.setProperty('--border', colors.border);
    st.setProperty(
        '--bg-2',
        `color-mix(in srgb, ${colors.sidebarBg} ${sideOpacity}%, transparent)`
    );
    st.setProperty('--sidebar-text', colors.sidebarText);
    st.setProperty('--bn-text', colors.bannerText);
    st.setProperty('--border-strong', `color-mix(in srgb, ${colors.border} 65%, ${colors.text})`);
    st.setProperty('--scroll', `color-mix(in srgb, ${colors.textDim} 32%, transparent)`);
    st.setProperty('--hover', `color-mix(in srgb, ${colors.text} ${dark ? 7 : 5}%, transparent)`);
    st.setProperty('--mask', dark ? 'rgba(2, 5, 6, 0.72)' : 'rgba(27, 36, 55, 0.42)');
    st.setProperty('--danger', dark ? '#ff6b73' : '#dc2626');
    st.setProperty('--danger-soft', dark ? 'rgba(255, 107, 115, 0.13)' : 'rgba(220, 38, 38, 0.08)');
    st.setProperty(
        '--danger-border',
        dark ? 'rgba(255, 107, 115, 0.34)' : 'rgba(220, 38, 38, 0.3)'
    );
    st.setProperty('--ok', dark ? '#68dc88' : '#168a42');
    st.setProperty('--ok-soft', dark ? 'rgba(104, 220, 136, 0.13)' : 'rgba(22, 138, 66, 0.1)');
    st.setProperty('--cyan', dark ? '#9ed7e9' : '#0e7490');
    st.setProperty('--cyan-soft', dark ? 'rgba(158, 215, 233, 0.13)' : 'rgba(14, 116, 144, 0.1)');
    st.setProperty(
        '--shadow',
        dark ? '0 2px 8px rgba(0, 0, 0, 0.16)' : '0 2px 8px rgba(31, 50, 85, 0.045)'
    );
    st.setProperty(
        '--shadow-lg',
        dark ? '0 20px 55px rgba(0, 0, 0, 0.42)' : '0 20px 55px rgba(31, 50, 85, 0.18)'
    );
    st.setProperty(
        '--shell-surface',
        `color-mix(in srgb, ${colors.bg} ${dark ? 30 : 42}%, transparent)`
    );
    st.setProperty('--glass-blur', '28px');
    // 图一布局是全部主题共享的固定骨架；旧 layout 字段只保留兼容，不再改变结构。
    st.setProperty('--sidebar-w', '208px');
    st.setProperty('--banner-h', '430px');
    st.setProperty('--radius', '14px');
}

/**
 * 所有正式主题共用玻璃变量与布局；个性化只替换用户色板。
 */
export function applyTheme(theme?: ThemeName, custom?: CustomTheme) {
    const root = document.documentElement;
    const selected = theme ?? 'transparent';
    clearCustomVars();
    root.dataset.theme = selected;
    if (selected === 'custom') {
        applyCustomVars(custom ?? DEFAULT_CUSTOM_THEME, selected);
        return;
    }
    const preset = THEME_PRESETS[selected] ?? THEME_PRESETS.transparent;
    applyCustomVars({ colors: preset.colors, layout: DEFAULT_CUSTOM_THEME.layout }, selected);
}

type AppearanceSource = () => Pick<Settings, 'theme' | 'custom'> | null | undefined;

export function installAppearanceRuntime(appearance: AppearanceSource): () => void {
    applyTheme(appearance()?.theme, appearance()?.custom);
    return watch(
        () => [appearance()?.theme, appearance()?.custom] as const,
        ([theme, custom]) => applyTheme(theme, custom),
        { immediate: true, deep: true }
    );
}
