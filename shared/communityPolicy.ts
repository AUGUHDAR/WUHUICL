import type { CommunityFile, CommunityKind, InstalledVersion, LoaderName } from './types';
import { compareVersions, normalizeLoader } from './modCompatibility';
import { compareMcVersions } from './keybindings';

export interface CommunityFileFilter {
    mcVersion?: string;
    loader?: LoaderName | '';
    kind?: CommunityKind;
}

/** Modrinth 用 loaders 同时标记模组加载器与资源文件类型。 */
export const MODRINTH_RESOURCE_LOADERS: Partial<Record<CommunityKind, string[]>> = {
    resourcepack: ['minecraft'],
    shader: ['iris', 'optifine', 'canvas', 'vanilla'],
    datapack: ['datapack']
};

export function usesCommunityLoader(kind?: CommunityKind): boolean {
    return !kind || kind === 'mod' || kind === 'modpack';
}

export function effectiveCommunityFilter(filter: CommunityFileFilter): CommunityFileFilter {
    return {
        ...filter,
        mcVersion: filter.mcVersion?.trim() || undefined,
        loader: usesCommunityLoader(filter.kind) ? filter.loader || undefined : undefined
    };
}

/**
 * 资源文件（资源包/光影包）的版本兼容：文件支持的版本列表里有 v 满足
 * v ≤ 实例版本 且同一 minor 线（1.21.x 任一 ≤ 1.21.4 即视为可用）。
 * 严格相等会把 Modrinth 的区间标记法（只标 1.21）误判为不支持。
 */
function resourceVersionCompatible(fileVersions: string[], instanceVersion: string): boolean {
    return fileVersions.some((v) => {
        const cmp = compareMcVersions(v, instanceVersion);
        if (cmp === 0) return true;
        if (cmp > 0) return false;
        // v < instance：同 minor 线（前面数字段完全一致，只多一个 patch）才兼容
        const a = v.split('.'),
            b = instanceVersion.split('.');
        return (
            a.length >= 2 &&
            b.length >= 2 &&
            a.slice(0, Math.max(1, a.length - 1)).join('.') ===
                b.slice(0, Math.max(1, a.length - 1)).join('.') &&
            a.length >= b.length
        );
    });
}

export function matchesCommunityFilter(file: CommunityFile, filter: CommunityFileFilter): boolean {
    filter = effectiveCommunityFilter(filter);
    const resourceLoaders = filter.kind && MODRINTH_RESOURCE_LOADERS[filter.kind];
    if (
        file.source === 'modrinth' &&
        resourceLoaders &&
        !file.loaders.some((l) => resourceLoaders.includes(l))
    )
        return false;
    const versionOk = !filter.mcVersion
        ? true
        : usesCommunityLoader(filter.kind)
          ? file.gameVersions.some((v) => compareVersions(v, filter.mcVersion!) === 0)
          : resourceVersionCompatible(file.gameVersions, filter.mcVersion);
    return (
        versionOk &&
        (!filter.loader ||
            file.loaders.some((l) => normalizeLoader(l) === normalizeLoader(filter.loader)))
    );
}

export function communityFileMatchesInstance(
    file: CommunityFile,
    instance: InstalledVersion
): boolean {
    return (
        !instance.failed &&
        !instance.incomplete &&
        !!instance.loader &&
        matchesCommunityFilter(file, { mcVersion: instance.mcVersion, loader: instance.loader })
    );
}
