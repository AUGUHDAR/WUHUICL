/** WUHUICL：原 @tauri-apps/api invoke → window.kamucl 桥（通道与 Rust schematic_preview::invoke 对应）。 */
function bridgeInvoke<T>(channel: string, payload?: unknown): Promise<T> {
    return (window as unknown as {
        kamucl: { invoke: (channel: string, ...args: unknown[]) => Promise<T> }
    }).kamucl.invoke(channel, payload ?? {})
}

/** 二进制响应经 JSON 通道时编码为 base64，这里解回 ArrayBuffer。 */
function decodeBase64(value: string): ArrayBuffer {
    const binary = atob(value)
    const bytes = new Uint8Array(binary.length)
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
    return bytes.buffer
}

export type SchematicPreviewSource =
    | { kind: 'external'; path: string }
    | { kind: 'instance'; instanceId: string; relativePath: string }
    | { kind: 'instance_file'; instanceId: string; relativePath: string }

export type SchematicBlockState = {
    name: string
    properties: Record<string, string>
}

export type SchematicChunkDescriptor = {
    position: [number, number, number]
    nonAirBlocks: number
}

export type SchematicRegion = {
    id: string
    name: string
    origin: [number, number, number]
    size: [number, number, number]
    min: [number, number, number]
    max: [number, number, number]
    blockCount: number
    chunks: SchematicChunkDescriptor[]
}

export type SchematicMaterial = {
    name: string
    count: number
}

export type SchematicPreviewManifest = {
    sessionId: string
    fileName: string
    sourcePath: string
    sourceInstanceId?: string
    format: 'litematic' | 'schem_v2' | 'schem_v3'
    formatVersion: number
    dataVersion?: number
    name?: string
    description?: string
    author?: string
    createdAt?: number
    modifiedAt?: number
    min: [number, number, number]
    max: [number, number, number]
    size: [number, number, number]
    blockCount: number
    entityCount: number
    blockEntityCount: number
    palette: SchematicBlockState[]
    materials: SchematicMaterial[]
    regions: SchematicRegion[]
    warnings: string[]
}

export type InstanceSchematicFile = {
    relativePath: string
    fileName: string
    format: 'litematic' | 'schem'
    size: number
    modifiedAt?: number
}

export type SchematicBlockEdit = {
    regionId: string
    position: [number, number, number]
    paletteIndex: number
}

export type SchematicChangedChunk = {
    regionId: string
    position: [number, number, number]
}

export type SchematicEditResult = {
    manifest: SchematicPreviewManifest
    changedChunks: SchematicChangedChunk[]
    appliedPaletteIndices: number[]
}

export type SchematicTransform =
    'rotate_clockwise' | 'rotate_counter_clockwise' | 'mirror_x' | 'mirror_z'

export async function openSchematicPreview(
    source: SchematicPreviewSource,
    requestId: string,
): Promise<SchematicPreviewManifest> {
    return await bridgeInvoke<SchematicPreviewManifest>(
        'schempreview:open',
        {
            source,
            requestId,
        },
    )
}

export async function listInstanceSchematics(instanceId: string): Promise<InstanceSchematicFile[]> {
    return await bridgeInvoke<InstanceSchematicFile[]>(
        'schempreview:listInstanceFiles',
        { instanceId },
    )
}

export async function readSchematicChunk(
    sessionId: string,
    regionId: string,
    position: [number, number, number],
): Promise<Uint32Array> {
    const response = decodeBase64(
        await bridgeInvoke<string>('schempreview:readChunk',
            {
                sessionId,
                regionId,
                position,
            },
        ),
    )
    const view = new DataView(response)
    if (
        response.byteLength !== 8 + 4096 * 4 ||
        view.getUint8(0) !== 0x53 ||
        view.getUint8(1) !== 0x50 ||
        view.getUint8(2) !== 0x43 ||
        view.getUint8(3) !== 0x31 ||
        view.getUint32(4, true) !== 4096
    ) {
        throw new Error('The schematic backend returned an invalid chunk.')
    }
    const result = new Uint32Array(4096)
    for (let index = 0; index < result.length; index += 1) {
        result[index] = view.getUint32(8 + index * 4, true)
    }
    return result
}

export async function getSchematicBlockInfo(
    sessionId: string,
    regionId: string,
    position: [number, number, number],
): Promise<SchematicBlockState | null> {
    return await bridgeInvoke<SchematicBlockState | null>(
        'schempreview:blockInfo',
        { sessionId, regionId, position },
    )
}

export async function applySchematicEdits(
    sessionId: string,
    edits: SchematicBlockEdit[],
    targetState?: SchematicBlockState,
): Promise<SchematicEditResult> {
    return await bridgeInvoke<SchematicEditResult>(
        'schempreview:applyEdits',
        { sessionId, edits, targetState: targetState ?? null },
    )
}

export async function exportSchematicSponge(sessionId: string): Promise<ArrayBuffer> {
    return decodeBase64(await bridgeInvoke<string>('schempreview:exportSponge', { sessionId }))
}

export async function exportSchematicLitematic(sessionId: string): Promise<ArrayBuffer> {
    return decodeBase64(
        await bridgeInvoke<string>('schempreview:exportLitematic', { sessionId }),
    )
}

export async function transformSchematic(
    sessionId: string,
    transform: SchematicTransform,
): Promise<SchematicPreviewManifest> {
    return await bridgeInvoke<SchematicPreviewManifest>(
        'schempreview:transform',
        { sessionId, transform },
    )
}

export async function closeSchematicPreview(sessionId: string): Promise<void> {
    await bridgeInvoke('schempreview:close', { sessionId })
}

export async function cancelSchematicPreview(requestId: string): Promise<void> {
    await bridgeInvoke('schempreview:cancel', { requestId })
}
