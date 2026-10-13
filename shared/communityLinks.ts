import type { CommunityResult } from './types'

/** 优先使用源站名称；旧结果只去掉中文翻译前缀，不把版本/Loader拼进搜索词。 */
export function mcmodSearchUrl(item: Pick<CommunityResult, 'title' | 'originalTitle' | 'slug'>): string | null {
  const raw = (item.originalTitle ?? item.title).trim()
  const title = raw.replace(/^[^|]*\p{Script=Han}[^|]*\|\s*/u, '')
  const english = /\p{Script=Han}/u.test(title) ? '' : title
  // 源站确实没有英文名时用英文 slug，不向百科发送中文名称。
  const keyword = /[a-z]/i.test(english) ? english : /^[a-z0-9_-]+$/i.test(item.slug) ? item.slug.replace(/[-_]+/g, ' ') : ''
  return keyword ? `https://search.mcmod.cn/s?key=${encodeURIComponent(keyword)}` : null
}
