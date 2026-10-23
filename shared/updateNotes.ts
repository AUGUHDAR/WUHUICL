/** 内置更新日志：下拉面板数据源（无网络依赖） */
export interface UpdateNote {
    version: string;
    /** 发布时间：YYYY-MM-DD HH:mm（24 小时制） */
    date: string;
    changes: string[];
}

/**
 * WUHUICL 版本线，按版本倒序；latest 为当前版本。
 * 与 src-tauri/Cargo.toml、tauri.conf.json 的 version 同步维护；发布新版本时在顶部追加。
 */
export const updateNotes: UpdateNote[] = [
    {
        version: '1.0.1',
        date: '2026-10-23 21:10',
        changes: [
            '新功能：外观设置新增「启动形象」——开屏头像与左上角图标可改为当前登录账号的皮肤（皮肤站/正版均支持，换号自动跟随），或上传自定义皮肤（64×64 PNG）',
            '更新源：更新下载源明确列出 Gitee 官方国内源，并新增「仅 Gitee（国内直连）」档位',
            '修复：按键设置「全部恢复默认」此前提示成功但实际未生效的问题',
            '交互：删除账号/删除外置登录提供商、按键全部恢复默认增加二次确认；顶栏通知/更新日志/下载中心支持 ESC 关闭；界面符号图标统一替换为矢量图标'
        ]
    },
    {
        version: '1.0.0',
        date: '2026-10-17 20:00',
        changes: [
            '基础：版本安装/启动/资源补全全链路可用；PCL 口径目录（exe 旁 .minecraft 与 WUHUICL 数据目录）；多候选下载引擎带分片并行、慢速换源与流式校验',
            'Java：Mojang 官方 runtime 自动下载（无 Java 环境也能玩），组件按游戏版本自动映射；隐藏本机 Java 亦可用',
            '外置登录：authlib-injector 全链路（Yggdrasil 认证/多角色选择/会话刷新/启动注入 + 系统根证书信任），LittleSkin 内置开箱即登录',
            '拖拽登录：皮肤站卡片拖进启动器直接弹登录框，一步登录入库；Windows 文本拖拽已修复（wry 补丁）',
            '皮肤：微软/外置/离线三通道皮肤与披风；离线默认史蒂夫；方块头像与 3D 预览；UUID 皮肤模型算法（PCL McSkinSex 同款）',
            '崩溃分析：移植 PCL 完整崩溃规则库，启动崩溃精确点名问题模组；启动挂死看门狗自动收尸并标注',
            '真实图标：加载器全套官方素材（Forge 铁砧/Fabric 布料/NeoForge/Quilt/OptiFine 徽章），皮肤站官方 logo 自动识别',
            '拖拽全家桶：MOD/资源包/光影/投影/整合包/存档/皮肤站卡片，拖进窗口即识别处理',
            '社区资源：Modrinth/CurseForge 检索与下载，MC 百科跳转，游戏版本/加载器筛选；下载统一进下载中心可取消',
            '联机：VoxLink P2P 地基 + 局域网直连/FRP 中转面板，网卡枚举与 UPnP 与原版对齐'
        ]
    }
];

export const latestUpdateNote = (): UpdateNote => updateNotes[0];
