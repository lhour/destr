//! 演示模式脚手架：`--demo` 无人值守跑 case + 截图 + 退出。
//!
//! 设计约束：演示模式必须**独立于用户交互系统**，只通过同一个"事件入口"
//! （比如 destructible_wall 的 BlockPunched 消息）推动场景，这样验证
//! 的就是真实玩家路径，不是一条"演示专属的假捷径"。
//!
//! case 想启用演示模式，只需：
//!   1. main 里 `if std::env::args().any(|a| a == "--demo")`
//!      追加系统：demo_drive + demo_shot + demo_exit
//!   2. demo_drive：按时间表调用 case 自己的交互入口（发消息）
//!   3. demo_shot：`request_screenshot` 存 `shots/<case_name>.png`
//!   4. demo_exit：调用 `request_exit`

use bevy::app::AppExit;
use bevy::ecs::prelude::*;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use std::path::PathBuf;

/// 演示统一状态。放进 case 的 setup 里 `.insert_resource(Default::default())`。
#[derive(Resource, Default)]
pub struct DemoDriver {
    pub actions_done: usize,
    pub shot_taken: bool,
}

/// 统一请求截图。文件名按 case 名字写进 shots/ 目录（repo 根目录）。
///
/// 用 `CARGO_MANIFEST_DIR`（`crate` 的 Cargo.toml 所在目录，即 repo root）
/// 拼绝对路径，避免"从不同工作目录运行，截图落到不同位置"的坑。
pub fn request_screenshot(commands: &mut Commands, case_name: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shots = root.join("shots");
    let _ = std::fs::create_dir_all(&shots);
    let path = shots.join(format!("{}_shot.png", case_name));
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path.to_string_lossy().to_string()));
}

/// 统一请求退出（case 的 demo_exit 系统用这个）。
pub fn request_exit(exit: &mut bevy::ecs::message::MessageWriter<AppExit>) {
    exit.write(AppExit::Success);
}

/// 打印一行标记：方便在日志里找"截图发生在哪个时间点"。
pub fn shot_marker(case: &str) {
    eprintln!("[demo::shot] {} → shots/{}_shot.png", case, case);
}
