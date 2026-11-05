<script setup lang="ts">
import {
    CodeIcon,
    ExternalIcon,
    ImageIcon,
    InfoIcon,
} from '../../../lab/icons'
import {
    Button,
    defineMessages,
    NewModal,
    useVIntl,
    tooltip as vTooltip,
} from '../../../lab/ui';
import '../../../lab/ui/ui.css';
import '../../../lab/ui/tw.css';
import { useTemplateRef } from 'vue'

const { formatMessage } = useVIntl()
const modal = useTemplateRef<InstanceType<typeof NewModal>>('modal')

const messages = defineMessages({
    title: {
        id: 'app.lab.seed-map.copyright.title',
        defaultMessage: '版权与来源说明',
    },
    iconHeading: {
        id: 'app.lab.seed-map.copyright.icons-heading',
        defaultMessage: '地图图稿',
    },
    iconBody: {
        id: 'app.lab.seed-map.copyright.icons-body',
        defaultMessage:
            '本工具使用的部分结构与群系图标取自 MinecraftSearch。相关图稿的权利仍归 MinecraftSearch 及相应创作者所有。',
    },
    visitMinecraftSearch: {
        id: 'app.lab.seed-map.copyright.visit-minecraft-search',
        defaultMessage: '访问 MinecraftSearch',
    },
    engineHeading: {
        id: 'app.lab.seed-map.copyright.engine-heading',
        defaultMessage: '本地地图引擎',
    },
    engineBody: {
        id: 'app.lab.seed-map.copyright.engine-body',
        defaultMessage:
            '群系、地形、出生点与结构数据由 cubiomes 和 Axolotl 原生集成在本地生成。cubiomes 版权所有 (c) 2020 Cubitect，并依据 MIT 许可证提供。',
    },
    viewCubiomes: {
        id: 'app.lab.seed-map.copyright.view-cubiomes',
        defaultMessage: '查看 cubiomes',
    },
    disclaimerHeading: {
        id: 'app.lab.seed-map.copyright.disclaimer-heading',
        defaultMessage: '非官方工具',
    },
    disclaimerBody: {
        id: 'app.lab.seed-map.copyright.disclaimer-body',
        defaultMessage:
            'Minecraft 是 Mojang Synergies AB 的商标。Axolotl Launcher 与 Mojang 或 MinecraftSearch 均无隶属关系，也未获得其认可。',
    },
})

defineExpose({
    show: (event?: MouseEvent) => modal.value?.show(event),
})
</script>

<template>
    <NewModal ref="modal" :header="formatMessage(messages.title)">
        <div class="copyright-notice">
            <section
                class="notice-section grid grid-cols-[1.5rem_minmax(0,1fr)] gap-3 border-b border-surface-5 pb-4"
            >
                <ImageIcon aria-hidden="true" />
                <div>
                    <h3>{{ formatMessage(messages.iconHeading) }}</h3>
                    <p>{{ formatMessage(messages.iconBody) }}</p>
                    <Button
                        type="outlined"
                        size="2xs"
                        @click="window.open('https://minecraftsearch.com', '_blank', 'noopener')"
                        >{{ formatMessage(messages.visitMinecraftSearch) }}
                        <ExternalIcon />
                    </Button>
                </div>
            </section>

            <section
                class="notice-section grid grid-cols-[1.5rem_minmax(0,1fr)] gap-3 border-b border-surface-5 pb-4"
            >
                <CodeIcon aria-hidden="true" />
                <div>
                    <h3>{{ formatMessage(messages.engineHeading) }}</h3>
                    <p>{{ formatMessage(messages.engineBody) }}</p>
                    <Button
                        type="outlined"
                        size="2xs"
                        @click="window.open('https://github.com/Cubitect/cubiomes', '_blank', 'noopener')"
                        >{{ formatMessage(messages.viewCubiomes) }}
                        <ExternalIcon />
                    </Button>
                </div>
            </section>

            <section
                class="notice-section grid grid-cols-[1.5rem_minmax(0,1fr)] gap-3 border-b border-surface-5 pb-4"
            >
                <InfoIcon aria-hidden="true" />
                <div>
                    <h3>{{ formatMessage(messages.disclaimerHeading) }}</h3>
                    <p>{{ formatMessage(messages.disclaimerBody) }}</p>
                </div>
            </section>
        </div>
    </NewModal>
</template>

<style scoped>
.copyright-notice {
    display: flex;
    width: min(34rem, calc(100vw - 3rem));
    flex-direction: column;
    gap: 1rem;
}

.notice-section:last-child {
    border-bottom: 0;
    padding-bottom: 0;
}

.notice-section > svg {
    width: 1.25rem;
    height: 1.25rem;
    margin-top: 0.1rem;
    color: var(--color-text-tertiary);
}

.notice-section h3 {
    margin: 0;
    color: var(--color-text-primary);
    font-size: 0.95rem;
}

.notice-section p {
    margin: 0.35rem 0 0.75rem;
    color: var(--color-text-tertiary);
    font-size: 0.8rem;
    line-height: 1.5;
}
</style>
