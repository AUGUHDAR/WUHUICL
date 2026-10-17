<script setup lang="ts">
import ThemeEditor from './ThemeEditor.vue';
</script>
<template>
    <details
        data-ui="SettingsView:24e434dcfefc"
        data-section="theme"
        class="card group collapse"
        open
    >
        <summary data-ui="SettingsView:f10bc27bda82" class="collapse-head">
            <h3 data-ui="SettingsView:12a109aabb6e" class="group-title">主题</h3>
            <span
                data-ui="SettingsView:08891c0d2242"
                class="collapse-arrow"
                aria-hidden="true"
            ></span>
        </summary>
        <div data-ui="SettingsView:23fb0346fede" class="collapse-body">
            <ThemeEditor />
        </div>
    </details>
</template>
<style scoped>
/* The moved theme editor owns the controls; this card only keeps the settings shell. */
.collapse {
    padding: 0;
}
.collapse-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    min-height: var(--row-h);
    padding: var(--space-2) var(--card-pad);
    cursor: pointer;
    list-style: none;
    user-select: none;
}
.collapse-head::-webkit-details-marker {
    display: none;
}
.collapse-arrow {
    width: 7px;
    height: 7px;
    border-right: 2px solid var(--text-dim);
    border-bottom: 2px solid var(--text-dim);
    transform: rotate(-45deg);
    transition: transform 0.18s ease;
    flex-shrink: 0;
}
.collapse[open] > .collapse-head .collapse-arrow {
    transform: rotate(45deg);
}
.collapse-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-3) var(--card-pad) var(--card-pad);
    border-top: 1px solid var(--border);
}
</style>
