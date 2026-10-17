import { createApp } from 'vue';
import type { Component } from 'vue';
import { installModalFocus } from '../../core/ui';
import { startIdleTrim } from '../../core/runtime';
import { platform } from '../../core/ipc';

export function bootstrapApp(root: Component, selector = '#app') {
    document.documentElement.dataset.platform = platform();
    const app = createApp(root);
    app.mount(selector);

    // Global lifecycle stays independent of view mounting and lazy route chunks.
    startIdleTrim();
    installModalFocus();
    return app;
}
