/** WUHUICL：原 @tauri-apps/api invoke → window.kamucl 桥（通道与 Rust seed_map::invoke 对应）。 */
function bridgeInvoke<T>(channel: string, payload?: unknown): Promise<T> {
    return (window as unknown as {
        kamucl: { invoke: (channel: string, ...args: unknown[]) => Promise<T> }
    }).kamucl.invoke(channel, payload ?? {})
}

/**
 * RGBA 原始字节经 JSON 通道时编码为 base64。解码 + ImageData + createImageBitmap
 * 全部丢进 Worker（atob 一片 ~350KB，缩小跨刻度时几十片集中到达，主线程解码是
 * 卡顿主因）；ImageBitmap 可 transfer 回主线程，零拷贝。Worker 起不来时退回主线程。
 */
let tileDecoder: Worker | null = null
let tileDecoderBroken = false
let decoderTokenSeq = 0
const decoderJobs = new Map<number, { resolve: (bitmap: ImageBitmap) => void; reject: (error: Error) => void }>()

function ensureTileDecoder(): Worker | null {
    if (tileDecoder || tileDecoderBroken) return tileDecoder
    try {
        tileDecoder = new Worker(new URL('./tile-decode.worker.ts', import.meta.url), { type: 'module' })
        tileDecoder.onmessage = (event: MessageEvent<{ token: number; bitmap?: ImageBitmap; error?: string }>) => {
            const job = decoderJobs.get(event.data.token)
            if (!job) return
            decoderJobs.delete(event.data.token)
            if (event.data.error || !event.data.bitmap) {
                job.reject(new Error(event.data.error ?? 'tile decode failed'))
            } else {
                job.resolve(event.data.bitmap)
            }
        }
        tileDecoder.onerror = () => {
            tileDecoderBroken = true
            tileDecoder = null
            for (const job of decoderJobs.values()) job.reject(new Error('tile decode worker crashed'))
            decoderJobs.clear()
        }
    } catch {
        tileDecoderBroken = true
        tileDecoder = null
    }
    return tileDecoder
}

function decodeTileBitmap(encoded: string, width: number, height: number): Promise<ImageBitmap> {
    const worker = ensureTileDecoder()
    if (!worker) return decodeTileBitmapOnMainThread(encoded, width, height)
    return new Promise<ImageBitmap>((resolve, reject) => {
        const token = ++decoderTokenSeq
        decoderJobs.set(token, { resolve, reject })
        worker.postMessage({ token, encoded, width, height })
    })
}

function decodeTileBitmapOnMainThread(encoded: string, width: number, height: number): Promise<ImageBitmap> {
    const binary = atob(encoded)
    const pixels = new Uint8ClampedArray(binary.length)
    for (let i = 0; i < binary.length; i++) pixels[i] = binary.charCodeAt(i)
    return createImageBitmap(new ImageData(pixels, width, height), {
        colorSpaceConversion: 'none',
        premultiplyAlpha: 'none',
    })
}

import type { SeedMapDimension } from './biomes.ts'
import type { SeedMapFeature, SeedMapFeatureKind } from './features.ts'
import type { SeedMapOreHit, SeedMapOreKind } from './ores.ts'

export type SeedMapEdition = 'java' | 'java-large-biomes'

export type SeedMapVersionProfile = {
    edition: SeedMapEdition
    version: string
    available: boolean
    dimensions: SeedMapDimension[]
    ores: boolean
    note?: string
}

export type SeedMapTileRequest = {
    epoch: number
    seed: string
    edition: SeedMapEdition
    version: string
    dimension: SeedMapDimension
    x: number
    z: number
    scale: number
    width: number
    height: number
    elevation?: number
    terrain: boolean
    contours: boolean
    highlightBiomes?: number[]
}

export type SeedMapTile = {
    epoch: number
    width: number
    height: number
    bitmap: ImageBitmap
    approximate: boolean
}

export type SeedMapFeatureQuery = {
    seed: string
    edition: SeedMapEdition
    version: string
    dimension: SeedMapDimension
    minX: number
    minZ: number
    maxX: number
    maxZ: number
    featureMask?: number
}

export type SeedMapSpawnPoint = {
    x: number
    z: number
    approximate: boolean
}

export type SeedMapLevelDat = {
    seed: string
    version?: string
}

export type SeedMapOreScanChunk = {
    cx: number
    cz: number
    hits: SeedMapOreHit[]
}

/**
 * Versions the bundled cubiomes engine understands, newest first. Releases
 * since the 1.21 winter drop (26.x and the late 1.21.x patches) share the
 * same world generation and all run on the newest bundled engine.
 */
export const SEED_MAP_FALLBACK_VERSIONS: readonly string[] = [
    '26.2',
    '26.1.2',
    '26.1',
    '1.21.9',
    '1.21.6',
    '1.21.5',
    '1.21.4',
    '1.21.3',
    '1.21.1',
    '1.20',
    '1.19.4',
    '1.19.2',
    '1.18',
    '1.17',
    '1.16',
    '1.15',
    '1.14',
    '1.13',
    '1.12',
    '1.11',
    '1.10',
    '1.9',
    '1.8',
    '1.7',
    '1.6',
    '1.5',
    '1.4',
    '1.3',
    '1.2',
    '1.1',
    '1.0',
]

export const SEED_MAP_DEFAULT_VERSION = SEED_MAP_FALLBACK_VERSIONS[0]

export function fallbackSeedMapProfiles(): SeedMapVersionProfile[] {
    const profiles: SeedMapVersionProfile[] = []
    for (const edition of ['java', 'java-large-biomes'] as const) {
        for (const version of SEED_MAP_FALLBACK_VERSIONS) {
            if (edition === 'java-large-biomes' && compareToMinor(version, 1, 3) < 0) continue
            const dimensions: SeedMapDimension[] = ['overworld']
            if (compareToMinor(version, 1, 16) >= 0) dimensions.push('nether')
            if (compareToMinor(version, 1, 9) >= 0) dimensions.push('end')
            profiles.push({
                edition,
                version,
                available: true,
                dimensions,
                ores: compareToMinor(version, 1, 18) >= 0,
            })
        }
    }
    return profiles
}

function compareToMinor(version: string, major: number, minor: number): number {
    const [versionMajor = 0, versionMinor = 0] = version.split('.').map(Number)
    if (versionMajor !== major) return versionMajor - major
    return versionMinor - minor
}

export async function getSeedMapProfiles(): Promise<SeedMapVersionProfile[]> {
    return await bridgeInvoke<SeedMapVersionProfile[]>('seedmap:profiles')
}

/**
 * Tiles render natively (one blocking thread each, no shared state); this cap
 * bounds how many renders are in flight at once. Decoding happens in a Worker,
 * so the real ceiling is CPU cores — keep it close to full saturation.
 */
export function seedMapTileConcurrency(
    scale: number,
    hardwareConcurrency = typeof navigator === 'undefined' ? 4 : navigator.hardwareConcurrency || 4,
): number {
    const cores = Math.max(1, Math.floor(hardwareConcurrency))
    const limit = Math.max(3, Math.min(12, cores - 1))
    return scale <= 1 ? Math.min(6, limit) : limit
}

export async function renderSeedMapTile(request: SeedMapTileRequest): Promise<SeedMapTile> {
    const encoded = await bridgeInvoke<string>('seedmap:renderTile', {
        seed: request.seed,
        edition: request.edition,
        version: request.version,
        dimension: request.dimension,
        x: request.x,
        z: request.z,
        scale: request.scale,
        width: request.width,
        height: request.height,
        elevation: request.elevation ?? 128,
        terrain: request.terrain,
        contours: request.contours,
        highlightBiomes: request.highlightBiomes ?? null,
    })
    const bitmap = await decodeTileBitmap(encoded, request.width, request.height)
    return {
        epoch: request.epoch,
        width: request.width,
        height: request.height,
        bitmap,
        approximate: request.terrain || request.contours,
    }
}

export async function findSeedMapFeatures(query: SeedMapFeatureQuery): Promise<SeedMapFeature[]> {
    const features = await bridgeInvoke<
        { kind: string; x: number; z: number; approximate: boolean; endShip?: boolean }[]
    >('seedmap:findFeatures', {
        seed: query.seed,
        edition: query.edition,
        version: query.version,
        dimension: query.dimension,
        minX: query.minX,
        minZ: query.minZ,
        maxX: query.maxX,
        maxZ: query.maxZ,
        featureMask: query.featureMask ?? null,
    })
    return features.map((feature) => ({
        kind: feature.kind as SeedMapFeatureKind,
        x: feature.x,
        z: feature.z,
        approximate: feature.approximate,
        endShip: feature.endShip,
    }))
}

export async function getSeedMapSpawn(
    seed: string,
    edition: SeedMapEdition,
    version: string,
): Promise<SeedMapSpawnPoint> {
    return await bridgeInvoke<SeedMapSpawnPoint>('seedmap:spawn', {
        seed,
        edition,
        version,
    })
}

export async function getSeedMapBiomeAt(query: {
    seed: string
    edition: SeedMapEdition
    version: string
    dimension: SeedMapDimension
    x: number
    y: number
    z: number
}): Promise<number> {
    return await bridgeInvoke<number>('seedmap:biomeAt', query)
}

export async function scanSeedMapOres(request: {
    seed: string
    version: string
    dimension: SeedMapDimension
    ores: SeedMapOreKind[]
    chunks: number[]
}): Promise<SeedMapOreScanChunk[]> {
    return await bridgeInvoke<SeedMapOreScanChunk[]>('seedmap:scanOres', request)
}

export async function readSeedMapLevelDat(path: string): Promise<SeedMapLevelDat> {
    return await bridgeInvoke<SeedMapLevelDat>('seedmap:readLevelDat', { path })
}
