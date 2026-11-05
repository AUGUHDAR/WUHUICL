import { createApp } from 'vue';
import App from './App.vue';
import { startIdleTrim } from './idleTrim';
import { initBridgePlatform } from './shim';
import './styles.css';
import './ui-system.css';
import { installModalFocus } from './modalFocus';
import { toast } from './store';

// 平台标识在挂载前就位：App.vue 的 isMac、CSS 的 html[data-platform] 都在首帧读取
void initBridgePlatform().then(() => {
    document.documentElement.dataset.platform = window.kamucl.platform;
    const app = createApp(App);
    // 单个视图崩溃不再静默白屏：控制台留全量栈，界面上至少给一条可见提示。
    // 注意：渲染期抛错的组件子树仍会缺失，这里只兜住"无声无息坏掉"的体验。
    app.config.errorHandler = (err, _instance, info) => {
        console.error('[wuhuicl] 界面组件出错', info, err);
        toast(
            '界面组件出错：' + (err instanceof Error ? err.message : String(err)) + `（${info}）`,
            'error'
        );
    };
    app.mount('#app');

    // 空闲瘦身挂在全局入口：不依赖任何视图生命周期，静默期暂停可重建缓存并触发 GC
    startIdleTrim();
    installModalFocus();
});
