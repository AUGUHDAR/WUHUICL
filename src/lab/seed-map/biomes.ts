/** WUHUICL：无 FormatJS，描述符原样保留（标签在页面里走中文映射）。 */
function defineMessages<T extends Record<string, { id: string; defaultMessage: string }>>(
    messages: T,
): T {
    return messages
}

export type SeedMapDimension = 'overworld' | 'nether' | 'end'

export type SeedMapBiomeCategory =
    | 'beach'
    | 'cave'
    | 'desert'
    | 'forest'
    | 'ice'
    | 'jungle'
    | 'mesa'
    | 'mountains'
    | 'mushroom'
    | 'ocean'
    | 'plains'
    | 'river'
    | 'savanna'
    | 'swamp'
    | 'taiga'
    | 'nether'
    | 'end'

export type SeedMapBiome = {
    id: number
    dimensions: readonly SeedMapDimension[]
    category: SeedMapBiomeCategory
    color: string
}

export type SeedMapBiomeGroup = {
    category: SeedMapBiomeCategory
    dimension: SeedMapDimension
    biomes: SeedMapBiome[]
}

const OVERWORLD: readonly SeedMapDimension[] = ['overworld']
const NETHER: readonly SeedMapDimension[] = ['nether']
const END: readonly SeedMapDimension[] = ['end']

/**
 * Java biome ids as used by cubiomes, grouped for the picker. Colors mirror
 * the native renderer's cubiomes palette so the picker and map stay aligned.
 */
export const SEED_MAP_BIOMES: readonly SeedMapBiome[] = [
    { id: 16, dimensions: OVERWORLD, category: 'beach', color: '#FADE55' },
    { id: 26, dimensions: OVERWORLD, category: 'beach', color: '#FAF0C0' },
    { id: 25, dimensions: OVERWORLD, category: 'beach', color: '#A2A284' },
    { id: 183, dimensions: OVERWORLD, category: 'cave', color: '#031F29' },
    { id: 174, dimensions: OVERWORLD, category: 'cave', color: '#4E3012' },
    { id: 175, dimensions: OVERWORLD, category: 'cave', color: '#283C00' },
    { id: 187, dimensions: OVERWORLD, category: 'cave', color: '#C8C828' },
    { id: 2, dimensions: OVERWORLD, category: 'desert', color: '#FA9418' },
    { id: 27, dimensions: OVERWORLD, category: 'forest', color: '#307444' },
    { id: 185, dimensions: OVERWORLD, category: 'forest', color: '#FF91C8' },
    { id: 29, dimensions: OVERWORLD, category: 'forest', color: '#40511A' },
    { id: 132, dimensions: OVERWORLD, category: 'forest', color: '#2D8E49' },
    { id: 4, dimensions: OVERWORLD, category: 'forest', color: '#056621' },
    { id: 178, dimensions: OVERWORLD, category: 'forest', color: '#47726C' },
    { id: 155, dimensions: OVERWORLD, category: 'forest', color: '#589C6C' },
    { id: 186, dimensions: OVERWORLD, category: 'forest', color: '#696D95' },
    { id: 34, dimensions: OVERWORLD, category: 'forest', color: '#5B7352' },
    { id: 181, dimensions: OVERWORLD, category: 'ice', color: '#B0B3CE' },
    { id: 11, dimensions: OVERWORLD, category: 'ice', color: '#A0A0FF' },
    { id: 140, dimensions: OVERWORLD, category: 'ice', color: '#B4DCDC' },
    { id: 168, dimensions: OVERWORLD, category: 'jungle', color: '#849500' },
    { id: 21, dimensions: OVERWORLD, category: 'jungle', color: '#507B0A' },
    { id: 23, dimensions: OVERWORLD, category: 'jungle', color: '#60930F' },
    { id: 37, dimensions: OVERWORLD, category: 'mesa', color: '#D94515' },
    { id: 165, dimensions: OVERWORLD, category: 'mesa', color: '#FF6D3D' },
    { id: 38, dimensions: OVERWORLD, category: 'mesa', color: '#B09765' },
    { id: 180, dimensions: OVERWORLD, category: 'mountains', color: '#DCDCC8' },
    { id: 177, dimensions: OVERWORLD, category: 'mountains', color: '#60A445' },
    { id: 179, dimensions: OVERWORLD, category: 'mountains', color: '#C4C4C4' },
    { id: 182, dimensions: OVERWORLD, category: 'mountains', color: '#7B8F74' },
    { id: 131, dimensions: OVERWORLD, category: 'mountains', color: '#888888' },
    { id: 3, dimensions: OVERWORLD, category: 'mountains', color: '#606060' },
    { id: 14, dimensions: OVERWORLD, category: 'mushroom', color: '#FF00FF' },
    { id: 46, dimensions: OVERWORLD, category: 'ocean', color: '#202070' },
    { id: 49, dimensions: OVERWORLD, category: 'ocean', color: '#202038' },
    { id: 50, dimensions: OVERWORLD, category: 'ocean', color: '#404090' },
    { id: 48, dimensions: OVERWORLD, category: 'ocean', color: '#000040' },
    { id: 24, dimensions: OVERWORLD, category: 'ocean', color: '#000030' },
    { id: 10, dimensions: OVERWORLD, category: 'ocean', color: '#7070D6' },
    { id: 45, dimensions: OVERWORLD, category: 'ocean', color: '#000090' },
    { id: 0, dimensions: OVERWORLD, category: 'ocean', color: '#000070' },
    { id: 44, dimensions: OVERWORLD, category: 'ocean', color: '#0000AC' },
    { id: 1, dimensions: OVERWORLD, category: 'plains', color: '#8DB360' },
    { id: 12, dimensions: OVERWORLD, category: 'plains', color: '#FFFFFF' },
    { id: 129, dimensions: OVERWORLD, category: 'plains', color: '#B5DB88' },
    { id: 7, dimensions: OVERWORLD, category: 'river', color: '#0000FF' },
    { id: 35, dimensions: OVERWORLD, category: 'savanna', color: '#BDB25F' },
    { id: 36, dimensions: OVERWORLD, category: 'savanna', color: '#A79D64' },
    { id: 163, dimensions: OVERWORLD, category: 'savanna', color: '#E5DA87' },
    { id: 184, dimensions: OVERWORLD, category: 'swamp', color: '#2CCC8E' },
    { id: 6, dimensions: OVERWORLD, category: 'swamp', color: '#07F9B2' },
    { id: 32, dimensions: OVERWORLD, category: 'taiga', color: '#596651' },
    { id: 160, dimensions: OVERWORLD, category: 'taiga', color: '#818E79' },
    { id: 30, dimensions: OVERWORLD, category: 'taiga', color: '#31554A' },
    { id: 5, dimensions: OVERWORLD, category: 'taiga', color: '#0B6A5F' },
    { id: 8, dimensions: NETHER, category: 'nether', color: '#572526' },
    { id: 170, dimensions: NETHER, category: 'nether', color: '#4D3A2E' },
    { id: 171, dimensions: NETHER, category: 'nether', color: '#981A11' },
    { id: 172, dimensions: NETHER, category: 'nether', color: '#49907B' },
    { id: 173, dimensions: NETHER, category: 'nether', color: '#645F63' },
    { id: 9, dimensions: END, category: 'end', color: '#8080FF' },
    { id: 40, dimensions: END, category: 'end', color: '#4B4BAB' },
    { id: 41, dimensions: END, category: 'end', color: '#C9C959' },
    { id: 42, dimensions: END, category: 'end', color: '#B5B536' },
    { id: 43, dimensions: END, category: 'end', color: '#7070CC' },
]

/** Display names for the Java biome ids shown in the picker. */
export const SEED_MAP_BIOME_NAMES: Readonly<Record<number, string>> = {
    0: 'Ocean',
    1: 'Plains',
    2: 'Desert',
    3: 'Windswept Hills',
    4: 'Forest',
    5: 'Taiga',
    6: 'Swamp',
    7: 'River',
    8: 'Nether Wastes',
    9: 'The End',
    10: 'Frozen Ocean',
    11: 'Frozen River',
    12: 'Snowy Plains',
    14: 'Mushroom Fields',
    16: 'Beach',
    21: 'Jungle',
    23: 'Sparse Jungle',
    24: 'Deep Ocean',
    25: 'Stony Shore',
    26: 'Snowy Beach',
    27: 'Birch Forest',
    29: 'Dark Forest',
    30: 'Snowy Taiga',
    32: 'Old Growth Pine Taiga',
    34: 'Windswept Forest',
    35: 'Savanna',
    36: 'Savanna Plateau',
    37: 'Badlands',
    38: 'Wooded Badlands',
    40: 'Small End Islands',
    41: 'End Midlands',
    42: 'End Highlands',
    43: 'End Barrens',
    44: 'Warm Ocean',
    45: 'Lukewarm Ocean',
    46: 'Cold Ocean',
    48: 'Deep Lukewarm Ocean',
    49: 'Deep Cold Ocean',
    50: 'Deep Frozen Ocean',
    129: 'Sunflower Plains',
    131: 'Windswept Gravelly Hills',
    132: 'Flower Forest',
    140: 'Ice Spikes',
    155: 'Old Growth Birch Forest',
    160: 'Old Growth Spruce Taiga',
    163: 'Windswept Savanna',
    165: 'Eroded Badlands',
    168: 'Bamboo Jungle',
    170: 'Soul Sand Valley',
    171: 'Crimson Forest',
    172: 'Warped Forest',
    173: 'Basalt Deltas',
    174: 'Dripstone Caves',
    175: 'Lush Caves',
    177: 'Meadow',
    178: 'Grove',
    179: 'Snowy Slopes',
    180: 'Jagged Peaks',
    181: 'Frozen Peaks',
    182: 'Stony Peaks',
    183: 'Deep Dark',
    184: 'Mangrove Swamp',
    185: 'Cherry Grove',
    186: 'Pale Garden',
    187: 'Sulfur Caves',
}

export function seedMapBiomeGroups(): SeedMapBiomeGroup[] {
    const groups = new Map<SeedMapBiomeCategory, SeedMapBiomeGroup>()
    for (const biome of SEED_MAP_BIOMES) {
        const dimension = biome.dimensions[0] ?? 'overworld'
        const group = groups.get(biome.category) ?? {
            category: biome.category,
            dimension,
            biomes: [],
        }
        group.biomes.push(biome)
        groups.set(biome.category, group)
    }
    return [...groups.values()]
}

export function seedMapBiomeSlug(name: string): string {
    return name.toLocaleLowerCase().replaceAll(' ', '-')
}

/** Static descriptors so FormatJS can extract biome labels (dynamic ids cannot). */
export const seedMapBiomeMessages = defineMessages({
    ocean: { id: 'app.lab.seed-map.biome.ocean', defaultMessage: '海洋' },
    plains: { id: 'app.lab.seed-map.biome.plains', defaultMessage: '平原' },
    desert: { id: 'app.lab.seed-map.biome.desert', defaultMessage: '沙漠' },
    'windswept-hills': {
        id: 'app.lab.seed-map.biome.windswept-hills',
        defaultMessage: '风袭丘陵',
    },
    forest: { id: 'app.lab.seed-map.biome.forest', defaultMessage: '森林' },
    taiga: { id: 'app.lab.seed-map.biome.taiga', defaultMessage: '针叶林' },
    swamp: { id: 'app.lab.seed-map.biome.swamp', defaultMessage: '沼泽' },
    river: { id: 'app.lab.seed-map.biome.river', defaultMessage: '河流' },
    'nether-wastes': {
        id: 'app.lab.seed-map.biome.nether-wastes',
        defaultMessage: '下界荒地',
    },
    'the-end': { id: 'app.lab.seed-map.biome.the-end', defaultMessage: '末地' },
    'frozen-ocean': {
        id: 'app.lab.seed-map.biome.frozen-ocean',
        defaultMessage: '冻洋',
    },
    'frozen-river': {
        id: 'app.lab.seed-map.biome.frozen-river',
        defaultMessage: '冻河',
    },
    'snowy-plains': {
        id: 'app.lab.seed-map.biome.snowy-plains',
        defaultMessage: '雪原',
    },
    'mushroom-fields': {
        id: 'app.lab.seed-map.biome.mushroom-fields',
        defaultMessage: '蘑菇岛',
    },
    beach: { id: 'app.lab.seed-map.biome.beach', defaultMessage: '沙滩' },
    jungle: { id: 'app.lab.seed-map.biome.jungle', defaultMessage: '丛林' },
    'sparse-jungle': {
        id: 'app.lab.seed-map.biome.sparse-jungle',
        defaultMessage: '稀疏丛林',
    },
    'deep-ocean': { id: 'app.lab.seed-map.biome.deep-ocean', defaultMessage: '深海' },
    'stony-shore': { id: 'app.lab.seed-map.biome.stony-shore', defaultMessage: '石岸' },
    'snowy-beach': { id: 'app.lab.seed-map.biome.snowy-beach', defaultMessage: '雪滩' },
    'birch-forest': {
        id: 'app.lab.seed-map.biome.birch-forest',
        defaultMessage: '白桦林',
    },
    'dark-forest': { id: 'app.lab.seed-map.biome.dark-forest', defaultMessage: '黑森林' },
    'snowy-taiga': { id: 'app.lab.seed-map.biome.snowy-taiga', defaultMessage: '雪原针叶林' },
    'old-growth-pine-taiga': {
        id: 'app.lab.seed-map.biome.old-growth-pine-taiga',
        defaultMessage: '原始松木针叶林',
    },
    'windswept-forest': {
        id: 'app.lab.seed-map.biome.windswept-forest',
        defaultMessage: '风袭森林',
    },
    savanna: { id: 'app.lab.seed-map.biome.savanna', defaultMessage: '热带草原' },
    'savanna-plateau': {
        id: 'app.lab.seed-map.biome.savanna-plateau',
        defaultMessage: '高原草原',
    },
    badlands: { id: 'app.lab.seed-map.biome.badlands', defaultMessage: '恶地' },
    'wooded-badlands': {
        id: 'app.lab.seed-map.biome.wooded-badlands',
        defaultMessage: '疏林恶地',
    },
    'small-end-islands': {
        id: 'app.lab.seed-map.biome.small-end-islands',
        defaultMessage: '末地小型岛屿',
    },
    'end-midlands': { id: 'app.lab.seed-map.biome.end-midlands', defaultMessage: '末地中部之地' },
    'end-highlands': {
        id: 'app.lab.seed-map.biome.end-highlands',
        defaultMessage: '末地高地',
    },
    'end-barrens': { id: 'app.lab.seed-map.biome.end-barrens', defaultMessage: '末地荒地' },
    'warm-ocean': { id: 'app.lab.seed-map.biome.warm-ocean', defaultMessage: '暖水海洋' },
    'lukewarm-ocean': {
        id: 'app.lab.seed-map.biome.lukewarm-ocean',
        defaultMessage: '温水海洋',
    },
    'cold-ocean': { id: 'app.lab.seed-map.biome.cold-ocean', defaultMessage: '冷水海洋' },
    'deep-lukewarm-ocean': {
        id: 'app.lab.seed-map.biome.deep-lukewarm-ocean',
        defaultMessage: '深海温水海洋',
    },
    'deep-cold-ocean': {
        id: 'app.lab.seed-map.biome.deep-cold-ocean',
        defaultMessage: '深海冷水海洋',
    },
    'deep-frozen-ocean': {
        id: 'app.lab.seed-map.biome.deep-frozen-ocean',
        defaultMessage: '深海冻洋',
    },
    'sunflower-plains': {
        id: 'app.lab.seed-map.biome.sunflower-plains',
        defaultMessage: '向日葵平原',
    },
    'windswept-gravelly-hills': {
        id: 'app.lab.seed-map.biome.windswept-gravelly-hills',
        defaultMessage: '风袭砂砾丘陵',
    },
    'flower-forest': {
        id: 'app.lab.seed-map.biome.flower-forest',
        defaultMessage: '繁花森林',
    },
    'ice-spikes': { id: 'app.lab.seed-map.biome.ice-spikes', defaultMessage: '冰刺之地' },
    'old-growth-birch-forest': {
        id: 'app.lab.seed-map.biome.old-growth-birch-forest',
        defaultMessage: '原始白桦林',
    },
    'old-growth-spruce-taiga': {
        id: 'app.lab.seed-map.biome.old-growth-spruce-taiga',
        defaultMessage: '原始云杉针叶林',
    },
    'windswept-savanna': {
        id: 'app.lab.seed-map.biome.windswept-savanna',
        defaultMessage: '风袭热带草原',
    },
    'eroded-badlands': {
        id: 'app.lab.seed-map.biome.eroded-badlands',
        defaultMessage: '被侵蚀的恶地',
    },
    'bamboo-jungle': {
        id: 'app.lab.seed-map.biome.bamboo-jungle',
        defaultMessage: '竹林',
    },
    'soul-sand-valley': {
        id: 'app.lab.seed-map.biome.soul-sand-valley',
        defaultMessage: '灵魂沙峡谷',
    },
    'crimson-forest': {
        id: 'app.lab.seed-map.biome.crimson-forest',
        defaultMessage: '绯红森林',
    },
    'warped-forest': {
        id: 'app.lab.seed-map.biome.warped-forest',
        defaultMessage: '诡异森林',
    },
    'basalt-deltas': {
        id: 'app.lab.seed-map.biome.basalt-deltas',
        defaultMessage: '玄武岩三角洲',
    },
    'dripstone-caves': {
        id: 'app.lab.seed-map.biome.dripstone-caves',
        defaultMessage: '溶洞',
    },
    'lush-caves': { id: 'app.lab.seed-map.biome.lush-caves', defaultMessage: '繁茂洞穴' },
    meadow: { id: 'app.lab.seed-map.biome.meadow', defaultMessage: '草甸' },
    grove: { id: 'app.lab.seed-map.biome.grove', defaultMessage: '雪林' },
    'snowy-slopes': {
        id: 'app.lab.seed-map.biome.snowy-slopes',
        defaultMessage: '雪坡',
    },
    'jagged-peaks': { id: 'app.lab.seed-map.biome.jagged-peaks', defaultMessage: '尖峭山峰' },
    'frozen-peaks': { id: 'app.lab.seed-map.biome.frozen-peaks', defaultMessage: '冰封山峰' },
    'stony-peaks': { id: 'app.lab.seed-map.biome.stony-peaks', defaultMessage: '裸岩山峰' },
    'deep-dark': { id: 'app.lab.seed-map.biome.deep-dark', defaultMessage: '深暗之域' },
    'mangrove-swamp': {
        id: 'app.lab.seed-map.biome.mangrove-swamp',
        defaultMessage: '红树林沼泽',
    },
    'cherry-grove': { id: 'app.lab.seed-map.biome.cherry-grove', defaultMessage: '樱花树林' },
    'pale-garden': { id: 'app.lab.seed-map.biome.pale-garden', defaultMessage: '苍白之园' },
    'sulfur-caves': { id: 'app.lab.seed-map.biome.sulfur-caves', defaultMessage: '硫磺洞穴' },
})

export function seedMapBiomeMessage(name: string) {
    return seedMapBiomeMessages[seedMapBiomeSlug(name) as keyof typeof seedMapBiomeMessages]
}
