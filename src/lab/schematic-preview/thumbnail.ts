/**
 * 投影行缩略预览：复用工坊的解析会话 + mesh worker + three.js，
 * 把 .litematic/.schem 渲染成小尺寸等轴测图（data URL）。
 * 串行队列 + 会话级缓存；仅在前端可见时入队（ProjectionThumb 组件）。
 */
import * as THREE from 'three'

import {
    closeSchematicPreview,
    openSchematicPreview,
    readSchematicChunk,
    type SchematicPreviewManifest,
    type SchematicPreviewSource,
} from './backend'
import { normalizeSchematicAirBlocks } from './editing'
import type {
    SchematicMeshData,
    SchematicMeshWorkerRequest,
    SchematicMeshWorkerResponse,
} from './mesh-worker'
import {
    createSchematicResources,
    minecraftVersionFromDataVersion,
    type LoadedSchematicResources,
} from './resources'

const THUMB_SIZE = 132
/** 超过这个方块量的投影不做缩略图（避免巨型文件卡住队列） */
const MAX_THUMBNAIL_BLOCKS = 2_000_000
/** 单个缩略图最多读取的区块数（按非空气块数量取前 N） */
const MAX_CHUNKS = 384

const cache = new Map<string, string>()
const failed = new Set<string>()
let inFlightId: string | null = null
let queue: Array<{
    id: string
    source: SchematicPreviewSource
    resolve: (dataUrl: string | null) => void
}> = []

export function cachedSchematicThumbnail(id: string): string | null {
    return cache.get(id) ?? null
}

/** 入队请求缩略图；完成（或失败）时回调一次。重复请求同一 id 直接走缓存。 */
export function requestSchematicThumbnail(
    id: string,
    source: SchematicPreviewSource,
    onDone: (dataUrl: string | null) => void
): void {
    const cached = cache.get(id)
    if (cached) {
        onDone(cached)
        return
    }
    if (failed.has(id)) {
        onDone(null)
        return
    }
    queue.push({
        id,
        source,
        resolve: (dataUrl) => {
            onDone(dataUrl)
        },
    })
    pump()
}

function pump(): void {
    if (inFlightId || queue.length === 0) return
    const job = queue.shift()!
    // 排队期间组件可能已引用同一 id 的更新来源；以先入队者为准
    inFlightId = job.id
    renderThumbnail(job.source)
        .then((dataUrl) => {
            if (dataUrl) cache.set(job.id, dataUrl)
            else failed.add(job.id)
            job.resolve(dataUrl)
        })
        .catch(() => {
            failed.add(job.id)
            job.resolve(null)
        })
        .finally(() => {
            inFlightId = null
            pump()
        })
}

// ---- 纹理资源按版本缓存（图集解码开销大）----
const resourcesCache = new Map<string, Promise<LoadedSchematicResources>>()

function resourcesFor(manifest: SchematicPreviewManifest): Promise<LoadedSchematicResources> {
    const version = minecraftVersionFromDataVersion(manifest.dataVersion) ?? '1.21.4'
    let entry = resourcesCache.get(version)
    if (!entry) {
        entry = createSchematicResources(version, manifest.palette)
        resourcesCache.set(version, entry)
        entry.catch(() => resourcesCache.delete(version))
    }
    return entry
}

async function renderThumbnail(source: SchematicPreviewSource): Promise<string | null> {
    const requestId = `thumb-${Math.random().toString(36).slice(2)}`
    const manifest = await openSchematicPreview(source, requestId)
    try {
        if (manifest.blockCount > MAX_THUMBNAIL_BLOCKS || !manifest.regions.length) {
            return null
        }
        const resources = await resourcesFor(manifest)
        const region = [...manifest.regions].sort(
            (left, right) => right.blockCount - left.blockCount
        )[0]
        const chunks = [...region.chunks]
            .filter((chunk) => chunk.nonAirBlocks > 0)
            .sort((left, right) => right.nonAirBlocks - left.nonAirBlocks)
            .slice(0, MAX_CHUNKS)
        if (!chunks.length) return null

        const group = await buildRegionMeshes(manifest, resources, region.id, chunks)
        if (!group.children.length) return null
        return renderIsoToDataUrl(group, region)
    } finally {
        void closeSchematicPreview(manifest.sessionId)
    }
}

function spawnMeshWorker(
    manifest: SchematicPreviewManifest,
    resources: LoadedSchematicResources
): { worker: Worker; ready: Promise<void> } {
    const worker = new Worker(new URL('./mesh-worker.ts', import.meta.url), { type: 'module' })
    const ready = new Promise<void>((resolve, reject) => {
        worker.onmessage = (event: MessageEvent<SchematicMeshWorkerResponse>) => {
            if (event.data.type === 'ready') resolve()
            else if (event.data.type === 'error') reject(new Error(event.data.message))
        }
        worker.onerror = (event) => reject(new Error(event.message))
    })
    worker.postMessage({
        type: 'init',
        epoch: 0,
        palette: manifest.palette,
        resources: resources.workerResources,
        seamlessGlass: false,
    } satisfies SchematicMeshWorkerRequest)
    return { worker, ready }
}

function meshChunk(
    worker: Worker,
    regionId: string,
    position: [number, number, number],
    blocks: ArrayBuffer
): Promise<{ opaque: SchematicMeshData; translucent: SchematicMeshData }> {
    return new Promise((resolve, reject) => {
        const jobId = `chunk-${Math.random().toString(36).slice(2)}`
        const onMessage = (event: MessageEvent<SchematicMeshWorkerResponse>) => {
            const message = event.data
            if (message.type !== 'mesh' || message.jobId !== jobId) return
            worker.removeEventListener('message', onMessage)
            worker.removeEventListener('error', onError)
            resolve({ opaque: message.opaque, translucent: message.translucent })
        }
        const onError = (event: ErrorEvent) => {
            worker.removeEventListener('message', onMessage)
            worker.removeEventListener('error', onError)
            reject(new Error(event.message))
        }
        worker.addEventListener('message', onMessage)
        worker.addEventListener('error', onError)
        worker.postMessage({
            type: 'mesh',
            epoch: 0,
            jobId,
            regionId,
            chunkPosition: position,
            blocks,
        } satisfies SchematicMeshWorkerRequest)
    })
}

async function buildRegionMeshes(
    manifest: SchematicPreviewManifest,
    resources: LoadedSchematicResources,
    regionId: string,
    chunks: Array<{ position: [number, number, number] }>
): Promise<THREE.Group> {
    const slot = spawnMeshWorker(manifest, resources)
    const group = new THREE.Group()
    try {
        await slot.ready
        for (const chunk of chunks) {
            const raw = await readSchematicChunk(manifest.sessionId, regionId, chunk.position)
            const blocks = normalizeSchematicAirBlocks(raw, manifest.palette).slice()
            const parts = await meshChunk(slot.worker, regionId, chunk.position, blocks.buffer)
            const pairs: Array<[SchematicMeshData, boolean]> = [
                [parts.opaque, false],
                [parts.translucent, true],
            ]
            for (const [data, translucent] of pairs) {
                if (!data.positions.length) continue
                const geometry = new THREE.BufferGeometry()
                geometry.setAttribute('position', new THREE.BufferAttribute(data.positions, 3))
                geometry.setAttribute('normal', new THREE.BufferAttribute(data.normals, 3))
                geometry.setAttribute('uv', new THREE.BufferAttribute(data.uvs, 2))
                geometry.setAttribute('color', new THREE.BufferAttribute(data.colors, 3))
                geometry.computeBoundingBox()
                geometry.computeBoundingSphere()
                group.add(new THREE.Mesh(geometry, makeMaterial(resources.texture, translucent)))
            }
        }
    } finally {
        slot.worker.terminate()
    }
    return group
}

function makeMaterial(texture: THREE.Texture, translucent: boolean): THREE.MeshLambertMaterial {
    const map = texture.clone()
    map.flipY = false
    map.magFilter = THREE.NearestFilter
    map.minFilter = THREE.NearestFilter
    map.generateMipmaps = false
    map.needsUpdate = true
    return new THREE.MeshLambertMaterial({
        map,
        vertexColors: true,
        alphaTest: translucent ? 0 : 0.08,
        transparent: translucent,
        opacity: translucent ? 0.68 : 1,
        depthWrite: !translucent,
    })
}

function renderIsoToDataUrl(
    group: THREE.Group,
    region: SchematicPreviewManifest['regions'][number]
): string | null {
    const scene = new THREE.Scene()
    scene.add(group)
    scene.add(new THREE.AmbientLight('#ffffff', 0.8))
    scene.add(new THREE.HemisphereLight('#87ceeb', '#444444', 0.6))
    const keyLight = new THREE.DirectionalLight('#fffbf0', 4)
    keyLight.position.set(-100, 151, 50)
    scene.add(keyLight)
    const fillLight = new THREE.DirectionalLight('#ffffff', 0.3)
    fillLight.position.set(-30, 40, -30)
    scene.add(fillLight)

    const size = new THREE.Vector3(...region.size)
    const center = new THREE.Vector3(
        (region.min[0] + region.max[0] + 1) / 2,
        (region.min[1] + region.max[1] + 1) / 2,
        (region.min[2] + region.max[2] + 1) / 2
    )
    const radius = Math.max(size.length() / 2, 1)

    const renderer = new THREE.WebGLRenderer({
        antialias: true,
        alpha: true,
        preserveDrawingBuffer: true,
    })
    renderer.setClearColor(0x000000, 0)
    renderer.setSize(THUMB_SIZE, THUMB_SIZE, false)

    const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, radius * 8 + 64)
    const half = radius * 1.18
    camera.left = -half
    camera.right = half
    camera.top = half
    camera.bottom = -half
    const direction = new THREE.Vector3(1, 0.82, 1).normalize()
    camera.position.copy(center).add(direction.multiplyScalar(radius * 4 + 32))
    camera.lookAt(center)
    camera.updateProjectionMatrix()

    try {
        renderer.render(scene, camera)
        return renderer.domElement.toDataURL('image/png')
    } finally {
        renderer.dispose()
        scene.traverse((object) => {
            if (object instanceof THREE.Mesh) {
                object.geometry.dispose()
                ;(object.material as THREE.Material).dispose()
            }
        })
    }
}
