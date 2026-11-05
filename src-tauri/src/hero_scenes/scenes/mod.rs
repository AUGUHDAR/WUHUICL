//! 场景注册表：Scene 枚举改自 LumilioCL v0.2.3 crates/lumilio-ui/src/hero/scenes/scene.rs
//! （AGPL-3.0）。改动：i18n `tr!` 换成硬编码中文 HUD 文案、轮播 ALL 含全部场景
//! （上游 Hearth 只属于 Continue 模式，本项目六个场景统一轮播）、新增 WUHUICL
//! 原创场景 Takeoff。要加自研场景：新写一个 scenes/<name>.rs 纯函数文件（签名
//! `pub fn render(grid: &mut PixelGrid, age: f32)`），在这里加枚举变体、render/
//! still_age/hud 三个分支，前端 src/heroScenes.ts 注册表加一行即可。

pub mod caves;
pub mod dawn;
pub mod hearth;
pub mod portal;
pub mod redstone;
pub mod takeoff;

use crate::hero_scenes::raster::PixelGrid;

pub use hearth::Landmark;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Scene {
    Dawn,
    Caves,
    Redstone,
    Portal,
    /// 夜营地营火；地标（远处山上的剪影）由当前选中实例决定。
    Hearth(Landmark),
    /// WUHUICL 原创场景：发射场夜航，火箭按周期点火升空。
    Takeoff,
}

impl Scene {
    /// 轮播顺序（前端 src/heroScenes.ts 的 HERO_SCENES 与此一致）。
    pub const ALL: [Self; 6] = [
        Self::Dawn,
        Self::Caves,
        Self::Redstone,
        Self::Portal,
        Self::Hearth(Landmark::Village),
        Self::Takeoff,
    ];

    /// Renders the scene. `flare` (0–1) is the pointer resting on 开始游戏;
    /// Hearth answers it with a taller fire.
    pub fn render(self, grid: &mut PixelGrid, age: f32, flare: f32) {
        match self {
            Self::Dawn => dawn::render(grid, age),
            Self::Caves => caves::render(grid, age),
            Self::Redstone => redstone::render(grid, age),
            Self::Portal => portal::render(grid, age),
            Self::Hearth(landmark) => hearth::render(grid, age, landmark, flare),
            Self::Takeoff => takeoff::render(grid, age, flare),
        }
    }

    /// A composed moment used when motion is reduced.
    pub const fn still_age(self) -> f32 {
        match self {
            Self::Dawn => 7.2,
            Self::Caves => 9.,
            Self::Redstone => 3.4,
            Self::Portal => 4.6,
            Self::Hearth(_) => 3.,
            Self::Takeoff => takeoff::STILL_AGE,
        }
    }

    /// 场景名（设置页/指示条提示用，与前端注册表一致）。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dawn => "黎明",
            Self::Caves => "洞穴",
            Self::Redstone => "红石",
            Self::Portal => "传送门",
            Self::Hearth(_) => "炉火",
            Self::Takeoff => "起飞",
        }
    }

    /// A live readout that tracks what the scene is doing right now. `size`
    /// is the texel grid the scene was last rasterised at.
    pub fn hud(self, age: f32, size: (i32, i32)) -> String {
        match self {
            Self::Dawn => format!("时间 {}", dawn::clock(age)),
            Self::Caves => {
                let cave = caves::Cave::new(size.0, size.1);
                format!("火把 ×{}", cave.torches_lit(age))
            }
            Self::Redstone => format!("信号强度 {}", redstone::signal(age)),
            Self::Portal => {
                if portal::charge(age) > 0.99 {
                    "已激活".to_owned()
                } else if age >= portal::IGNITE_AT {
                    "点火中".to_owned()
                } else {
                    "未点燃".to_owned()
                }
            }
            Self::Hearth(_) => format!("营火光照 {}", hearth::FIRE_LEVEL),
            Self::Takeoff => takeoff::hud(age),
        }
    }
}
