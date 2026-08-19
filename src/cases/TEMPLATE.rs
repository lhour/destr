//! 模板：最小 case 骨架。
//!
//! 用法：
//! ```bash
//! cp src/cases/TEMPLATE.rs src/cases/my_case.rs
//! # 编辑 Cargo.toml 加一条 [[bin]] 指向新文件
//! ```
//!
//! TODO 列表：
//! [ ] 设置 `CASE_NAME` 常量
//! [ ] 改 `App::new()` 里的 `add_systems`（StartUp / Update）
//! [ ] 实现 demo 自动播放逻辑（`demo_fire` / `demo_shot` / `demo_exit`）
//! [ ] 用 `--demo` 模式跑通，截图放进 shots/

use bevy::prelude::*;
use destr::common::{add_default_plugins, spawn_default_camera, spawn_ground, spawn_sun};
use destr::demo::{request_exit, request_screenshot, DemoDriver};

const CASE_NAME: &str = "TEMPLATE";

fn main() {
    // 读参数：是否 --demo
    let demo_mode = std::env::args().any(|a| a == "--demo");

    let mut app = App::new();
    add_default_plugins(&mut app, CASE_NAME);
    if demo_mode {
        app.insert_resource(DemoDriver::new());
    }

    app.add_systems(Startup, (setup_simple_scene,));

    // TODO: 加你自己的 Update 系统，交互系统和演示共用事件入口
    if demo_mode {
        app.add_systems(Update, (demo_fire, demo_shot, demo_exit).chain());
    }

    app.run();
}

fn setup_simple_scene(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    // 共享脚手架：阳光 + 相机 + 地面
    spawn_sun(&mut commands, 60.0, 30.0);
    spawn_default_camera(&mut commands, Vec3::new(6.0, 5.0, 8.0), Vec3::ZERO);
    spawn_ground(&mut commands, &mut meshes, &mut materials);

    // TODO: 这里放 case 的具体场景搭建
}

// ===== 演示模式三个系统：fire → shot → exit =====

fn demo_fire(time: Res<Time>, mut driver: ResMut<DemoDriver>, mut _commands: Commands) {
    // TODO: 替换成真实事件/触发（例如发一条消息让 apply_xxx 系统吃）
    const T1: f32 = 0.5;
    if driver.actions_done < 1 && time.elapsed_secs() > T1 {
        // commands.trigger(...) 或者 commands.send_event(...)
        driver.actions_done += 1;
        driver.at = time.elapsed_secs();
    }
}

fn demo_shot(time: Res<Time>, driver: Res<DemoDriver>, mut commands: Commands) {
    if driver.actions_done >= 1 && !driver.shot_taken && time.elapsed_secs() - driver.at > 0.6 {
        request_screenshot(&mut commands, CASE_NAME);
    }
}

fn demo_exit(
    images: Res<Assets<Image>>,
    mut driver: ResMut<DemoDriver>,
    mut exit: bevy::ecs::message::MessageWriter<AppExit>,
) {
    if driver.shot_taken && !images.is_empty() {
        // TODO: 加你这个 case 的验证（例如断言/打印某种"确实生效了"的指标）
        request_exit(&mut exit);
    }
}
