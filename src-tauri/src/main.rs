#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 投影工坊子进程模式：stdio JSON Lines 服务，与 Tauri 运行时完全无关。
    // 会话清空 / 主进程退出时自动结束（见 schem_worker）。
    if std::env::args().any(|arg| arg == "--schem-worker") {
        wuhuicl::schem_worker::run();
    }
    wuhuicl::run()
}
