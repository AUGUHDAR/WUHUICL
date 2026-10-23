/**
 * 皮肤站（第三方 Yggdrasil 认证服务）真实图标匹配。
 * 图标素材来自各皮肤站官方 favicon，落 public/loaders/skinsites/<slug>.png。
 * 按 provider 的 apiRoot / 名称做 hostname 匹配，未收录的站回退字母占位。
 */

const SITE_ICONS: Array<{ host: string; slug: string }> = [
    { host: 'littleskin.cn', slug: 'littleskin' }
];

function hostnameOf(url: string): string {
    try {
        return new URL(url).hostname.toLowerCase();
    } catch {
        // 宽松兜底：截 host 段（apiRoot 一般是规范 URL，不会走到这）
        const m = url.match(/^https?:\/\/([^/?#]+)/i);
        return m ? m[1].toLowerCase() : '';
    }
}

/** provider（apiRoot + 名称）→ 图标 URL | null */
export function skinSiteIconUrl(apiRoot: string | undefined | null, name?: string): string | null {
    const host = apiRoot ? hostnameOf(apiRoot) : '';
    for (const site of SITE_ICONS) {
        if (host === site.host || host.endsWith(`.${site.host}`)) {
            return `loaders/skinsites/${site.slug}.png`;
        }
    }
    // apiRoot 没命中时按站名兜底（如拖拽 JSON 自定义了不同 apiRoot 域名）
    const lower = (name ?? '').toLowerCase().replace(/\s+/g, '');
    for (const site of SITE_ICONS) {
        if (lower && lower.includes(site.host.split('.')[0])) {
            return `loaders/skinsites/${site.slug}.png`;
        }
    }
    return null;
}
