fn main() {
    build_cubiomes();
    tauri_build::build()
}

/// 种子地图：编译 vendor/cubiomes（Axolotl 同款固定提交）+ seed_map/cubiomes_bridge.c。
/// 结构对齐 Axolotl apps/app/build.rs：仅参与 seed_map 模块，桌面/Android 通用。
fn build_cubiomes() {
    // 纯类型检查用逃生门：在非 macOS 宿主机上 `cargo check --target *-apple-darwin`
    // 没有目标 C 工具链，cubiomes 编译必然失败。置 WUHUICL_SKIP_CUBIOMES=1 跳过编译，
    // 只保留 Rust 侧类型检查（产物无法链接，仅供 check）。真机/GitHub Actions 构建不要设。
    println!("cargo:rerun-if-env-changed=WUHUICL_SKIP_CUBIOMES");
    if std::env::var("WUHUICL_SKIP_CUBIOMES").as_deref() == Ok("1") {
        println!("cargo:warning=WUHUICL_SKIP_CUBIOMES=1：跳过 cubiomes 编译（仅类型检查）");
        return;
    }
    let cubiomes_dir = std::path::Path::new("vendor/cubiomes");
    cc::Build::new()
        .include(cubiomes_dir)
        .include(cubiomes_dir.join("tables"))
        .file("src/seed_map/cubiomes_bridge.c")
        .files([
            "vendor/cubiomes/biomenoise.c",
            "vendor/cubiomes/biomes.c",
            "vendor/cubiomes/finders.c",
            "vendor/cubiomes/generator.c",
            "vendor/cubiomes/layers.c",
            "vendor/cubiomes/noise.c",
            "vendor/cubiomes/quadbase.c",
            "vendor/cubiomes/util.c",
        ])
        .opt_level(3)
        .flag_if_supported("-fwrapv")
        .warnings(false)
        .compile("wuhuicl-cubiomes");

    println!("cargo:rerun-if-changed=src/seed_map/cubiomes_bridge.c");
    println!("cargo:rerun-if-changed=src/seed_map/cubiomes_bridge.h");
    println!("cargo:rerun-if-changed=vendor/cubiomes");
    // 构建脚本跑在宿主机上，按目标三元组决定是否链 libm
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        println!("cargo:rustc-link-lib=m");
    }
}
