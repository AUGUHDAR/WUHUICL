import type { CommunityResult } from './types';

/** 优先使用源站名称；旧结果只去掉中文翻译前缀，不把版本/Loader拼进搜索词。 */
export function mcmodSearchUrl(
    item: Pick<CommunityResult, 'title' | 'originalTitle' | 'slug'>
): string | null {
    const raw = (item.originalTitle ?? item.title).trim();
    const title = raw.replace(/^[^|]*\p{Script=Han}[^|]*\|\s*/u, '');
    const english = /\p{Script=Han}/u.test(title) ? '' : title;
    // 源站确实没有英文名时用英文 slug，不向百科发送中文名称。
    const keyword = /[a-z]/i.test(english)
        ? english
        : /^[a-z0-9_-]+$/i.test(item.slug)
          ? item.slug.replace(/[-_]+/g, ' ')
          : '';
    return keyword ? `https://search.mcmod.cn/s?key=${encodeURIComponent(keyword)}` : null;
}

/**
 * 百科链接：词典/百科桥命中的条目带 mcmodId，直达 class 详情页（PCL PageDownloadCompDetail
 * 同款口径 `mcmod.cn/class/{id}.html`）；无身份时退回搜索页。
 */
export function mcmodPageUrl(
    item: Pick<CommunityResult, 'title' | 'originalTitle' | 'slug' | 'mcmodId'>
): string | null {
    if (item.mcmodId && item.mcmodId > 0) {
        return `https://www.mcmod.cn/class/${item.mcmodId}.html`;
    }
    return mcmodSearchUrl(item);
}
