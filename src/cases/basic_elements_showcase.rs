//! Case 02: basic_elements_showcase —— 场景基础元素陈列。
//!
//! 把三个当前内置元素（Brick / CementBlock / DebrisPiece）按 X 轴排一列，
//! 每个元素后面立一块"1 单位宽 × 1 单位高 × 0.1 厚"的白色背板，
//! 方便肉眼对比元素颜色和尺寸。
//!
//! 尺寸 API 验证：
//!   - Brick / CementBlock / DebrisPiece 的尺寸常量
//!   - 实例方法 .get_length() / .get_length_x() / .get_width() ...
//! 这些都会在 `--demo` 模式退出前打印并做一致性断言。
//!
//! 运行：
//!   cargo run --bin basic_elements_showcase
//!   cargo run --bin basic_elements_showcase -- --demo

use bevy::app::AppExit;
use bevy::ecs::message::MessageWriter;
use bevy::prelude::*;

use destr::common::{
    add_default_plugins, plain_material, spawn_camera_at, spawn_ground, spawn_sun,
};
use destr::demo::{request_exit, request_screenshot, DemoDriver};
use destr::elements::{Brick, CementBlock, DebrisPiece, Element};

const CASE: &str = "basic_elements_showcase";
const ITEM_SPACING: f32 = 3.2;

fn main() {
    let demo = std::env::args().any(|a| a == "--demo");
    let mut app = App::new();
    add_default_plugins(&mut app, format!("destr · {CASE} — 基础元素陈列"));
    if demo {
        app.insert_resource(DemoDriver::default());
    }
    app.add_systems(Startup, setup);
    if demo {
        app.add_systems(Update, (demo_shot, demo_exit));
    }
    app.run();
}

struct Item {
    cn: &'static str,
}
const ITEMS: &[Item] = &[
    Item { cn: "标准砖" },
    Item { cn: "水泥块" },
    Item { cn: "碎砖(基准)" },
];

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    spawn_sun(&mut commands);
    let plain = plain_material(&mut materials);
    spawn_ground(&mut commands, &mut meshes, plain.clone());
    // 镜头近一点，居中看三个
    spawn_camera_at(&mut commands, Vec3::new(0.0, 3.0, 8.2), Vec3::new(0.0, 1.0, 0.0));

    // 一块公共的 1.2×1.2×0.1 背板 mesh：和每个元素同中心 z=-0.6 对齐
    let backer_mesh = meshes.add(Cuboid::new(1.2, 1.2, 0.1).mesh().build());
    let backer_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.96, 0.95, 0.92),
        base_color_texture: None,
        perceptual_roughness: 0.98,
        ..default()
    });

    let n = ITEMS.len();
    let span = (n as f32 - 1.0) * ITEM_SPACING;
    for (i, item) in ITEMS.iter().enumerate() {
        let x = -span / 2.0 + i as f32 * ITEM_SPACING;
        match i {
            0 => spawn_one::<Brick>(
                &mut commands, &mut meshes, &mut materials, &mut images,
                x, item.cn, backer_mesh.clone(), backer_mat.clone(),
            ),
            1 => spawn_one::<CementBlock>(
                &mut commands, &mut meshes, &mut materials, &mut images,
                x, item.cn, backer_mesh.clone(), backer_mat.clone(),
            ),
            2 => spawn_one::<DebrisPiece>(
                &mut commands, &mut meshes, &mut materials, &mut images,
                x, item.cn, backer_mesh.clone(), backer_mat.clone(),
            ),
            _ => unreachable!(),
        }
    }
}

/// spawn 一个元素本体 + 一块背板
fn spawn_one<E: Element>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    center_x: f32,
    _cn: &'static str,
    backer_mesh: Handle<Mesh>,
    backer_mat: Handle<StandardMaterial>,
) {
    // 元素本体：落在地面上
    let pos = Vec3::new(center_x, E::HEIGHT * 0.5, 0.0);
    let mesh_h = meshes.add(<E as Element>::base_mesh());
    let mat_h = <E as Element>::default_material(materials, images, Color::WHITE, 0.93);
    commands.spawn((
        Mesh3d(mesh_h),
        MeshMaterial3d(mat_h),
        Transform::from_translation(pos),
        Name::new(format!("element_{}", E::NAME)),
    ));

    // 背板：放在元素后 0.6 处，高度 0.6 对齐元素中心（Y 向在 0.6）
    commands.spawn((
        Mesh3d(backer_mesh),
        MeshMaterial3d(backer_mat),
        Transform::from_translation(Vec3::new(center_x, 0.6, -0.6)),
        Name::new(format!("backer_{}", E::NAME)),
    ));
}

// ── 演示模式 ───────────────────────────────────────────────────────

fn demo_shot(time: Res<Time>, mut demo: ResMut<DemoDriver>, mut commands: Commands) {
    if time.elapsed_secs() >= 0.6 && !demo.shot_taken {
        demo.shot_taken = true;
        destr::demo::shot_marker(CASE);
        request_screenshot(&mut commands, CASE);
    }
}

fn demo_exit(time: Res<Time>, mut exit: MessageWriter<AppExit>) {
    if time.elapsed_secs() > 2.0 {
        println!("\n=== [{CASE}] 演示结束：展示 {n} 个基础元素 ===", n = ITEMS.len());
        let list: [(&str, &str, Vec3); 3] = [
            ("标准砖",     Brick::NAME,       Brick::SIZE),
            ("水泥块",     CementBlock::NAME, CementBlock::SIZE),
            ("碎砖(基准)", DebrisPiece::NAME, DebrisPiece::SIZE),
        ];
        for (cn, en, s) in list {
            println!("  · {:<10}  NAME={:<14}  SIZE=({:.2}, {:.2}, {:.2})", cn, en, s.x, s.y, s.z);
        }

        // 验证：实例方法 get_length() == 常量 SIZE
        let b = Brick;
        let ok1 = (b.get_length() - Brick::SIZE).length_squared() < 1e-6;
        let ok2 = b.get_length_x() == Brick::WIDTH
            && b.get_length_y() == Brick::HEIGHT
            && b.get_length_z() == Brick::DEPTH;
        // 验证：get_width/height/depth 和 get_length_xyz 是同一组值
        let ok3 = b.get_width() == b.get_length_x()
            && b.get_height() == b.get_length_y()
            && b.get_depth() == b.get_length_z();
        // 验证：get_size() == get_length()
        let ok4 = (b.get_size() - b.get_length()).length_squared() < 1e-6;
        let all = ok1 && ok2 && ok3 && ok4;
        println!("  · Brick API 一致性 (SIZE↔get_length / get_length_x / get_width↔_x / get_size↔_length): {}", if all { "✓ PASS" } else { "✗ FAIL" });
        assert!(all, "Element API 一致性失败: ok1={ok1} ok2={ok2} ok3={ok3} ok4={ok4}");

        request_exit(&mut exit);
    }
}
