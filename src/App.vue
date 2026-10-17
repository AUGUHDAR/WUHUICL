<script setup lang="ts">
import { provide } from 'vue';
import AppShell from './app/shell/AppShell.vue';
import { APP_SHELL_CONTEXT, useAppShell } from './app/shell/useAppShell';

const shell = useAppShell();
provide(APP_SHELL_CONTEXT, shell);
</script>

<template>
    <AppShell />
</template>
