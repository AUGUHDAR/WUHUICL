<script setup lang="ts">
// WUHUICL：单人署名（原版为 KAMUCL 官方测试与创作者名单）
const names = ['物晖'];
const CREATOR_SPACE = 'https://space.bilibili.com/526277131';
const ISSUES_URL = 'https://github.com/AUGUHDAR/WUHUICL';
/** 用户实测两次否决悬浮形态变化：本卡不做任何 hover 展开/位移。
 *  旧实现的根因：pointerenter 置 expanded → .creator-reveal 由 grid-template-rows
 *  0fr 过渡到 1fr，卡片高度 +~54px；首页 1360×860 下 .home-side 与 .home-main 底边
 *  恰好齐平，右栏变高即成为 dashboard 行高来源 → instances-block(flex:1) 被重新分高，
 *  「最近游戏」整块随之下拉。现展开层已整体删除，卡片高度恒定。 */
function openSpace() {
    window.open(CREATOR_SPACE, '_blank', 'noopener,noreferrer');
}
</script>
<template>
    <section class="creator-card" data-ui="home:creators" @click="openSpace">
        <!-- 标题行 = 创作者 CREATOR + 物晖胶囊 + ↗ 外链，一行排布（用户实测修正 #3） -->
        <div data-ui="CreatorCard:3b0bd35ff2cd" class="creator-heading">
            <button
                data-ui="CreatorCard:b838c4453411"
                class="creator-toggle"
                type="button"
                title="打开物晖的哔哩哔哩空间"
            >
                创作者<small data-ui="CreatorCard:d9b60520217e" lang="en">CREATOR</small>
            </button>
            <span
                data-ui="CreatorCard:7cd0fbc9196c"
                v-for="name in names"
                :key="name"
                class="creator-bubble"
                ><span data-ui="CreatorCard:587246bec0b9">{{ name }}</span></span
            >
            <a
                data-ui="CreatorCard:86b4c178a448"
                :href="CREATOR_SPACE"
                target="_blank"
                rel="noopener noreferrer"
                class="icon-btn creator-external"
                aria-label="访问物晖的哔哩哔哩空间"
                @click.stop
                >↗</a
            >
        </div>
        <div class="creator-message">
            <p data-ui="CreatorCard:5031cfb3dd7e" class="creator-intro">
                如果你有任何的想反馈的BUG或者好的想法，欢迎去B站、QQ、issues(<a
                    data-ui="CreatorCard:5031cfb3dd7f"
                    class="creator-link"
                    :href="ISSUES_URL"
                    target="_blank"
                    rel="noopener noreferrer"
                    @click.stop
                    >https://github.com/AUGUHDAR/WUHUICL</a
                >)等地方联系物晖，也欢迎提交PR！
            </p>
        </div>
    </section>
</template>
<style scoped>
.creator-card {
    min-width: 0;
    overflow: hidden;
    padding: var(--card-pad);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-content);
    cursor: pointer;
}
.creator-heading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
}
.creator-toggle {
    color: var(--text);
    background: none;
    border: 0;
    padding: 0;
    text-align: left;
    font-size: var(--text-md);
    font-weight: 600;
    cursor: pointer;
}
.creator-toggle small {
    display: block;
    font-size: 11px;
    letter-spacing: 1px;
    color: var(--text-dim);
    margin-top: 4px;
}
/* 署名胶囊：从正文下的悬浮展开层移到标题行（正文不再带署名行） */
.creator-bubble {
    flex: none;
    font-size: 12px;
    border-radius: 999px;
    background: var(--card-2);
    padding: 4px 10px;
}
.creator-external {
    flex: none;
    margin-left: auto;
    color: var(--text-dim);
    text-decoration: none;
}
.creator-external:hover {
    color: var(--text);
}
.creator-intro {
    font-size: 12px;
    line-height: 1.7;
    color: var(--text-dim);
    margin: var(--space-3) 0 0;
}
.creator-link {
    color: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
    word-break: break-all;
}
</style>
