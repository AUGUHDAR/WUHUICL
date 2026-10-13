import type { CustomTheme } from './types'

const rgb = (hex: string) => [1, 3, 5].map(index => parseInt(hex.slice(index, index + 2), 16))
function luminance(hex: string): number {
  return rgb(hex).map(value => value / 255).map(value => value <= .04045 ? value / 12.92 : ((value + .055) / 1.055) ** 2.4)
    .reduce((sum, value, index) => sum + value * [.2126, .7152, .0722][index], 0)
}
export function colorContrast(a: string, b: string): number {
  const values = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (values[0] + .05) / (values[1] + .05)
}
function mix(a: string, b: string, weight: number): string {
  const right = rgb(b)
  return '#' + rgb(a).map((value, index) => Math.round(value * weight + right[index] * (1 - weight)).toString(16).padStart(2, '0')).join('')
}
/** 渲染时修正旧主题的明暗冲突，不改写用户保存的配色。 */
export function readableCustomColors(input: CustomTheme['colors']): CustomTheme['colors'] {
  const colors = { ...input }
  const dark = luminance(colors.bg) < .18
  // 窗口和卡片共用文字色；避免旧配置白卡片/深色背景混用而无法阅读。
  for (const key of ['card', 'sidebarBg'] as const) {
    if ((luminance(colors[key]) < .18) !== dark) colors[key] = mix(colors[key], colors.bg, .15)
  }
  const readable = (value: string, backgrounds: string[]) => {
    const score = (color: string) => Math.min(...backgrounds.map(bg => colorContrast(color, bg)))
    if (score(value) >= 4.5) return value
    return score('#ffffff') > score('#101218') ? '#ffffff' : '#101218'
  }
  colors.text = readable(colors.text, [colors.bg, colors.card])
  colors.textDim = readable(colors.textDim, [colors.bg, colors.card])
  colors.sidebarText = readable(colors.sidebarText, [colors.bg, colors.sidebarBg])
  return colors
}
