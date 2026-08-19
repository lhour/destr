//! Case 02: basic_elements_showcase —— 场景基础元素陈列。
//!
//! 当前陈列 6 种元素（按 X 轴等间距排成一行）：
//!   0. Brick                 标准砖（1.00 × 0.50 × 0.60）
//!   1. CementBlock           水泥块（1.00 × 0.60 × 0.60）
//!   2. DebrisPiece           碎砖 / 基准（0.22³，无边框纯脏噪点贴图）
//!   3. IrregularRock         不规则石块（抖动顶点多面体 + rock_texture）
//!   4. ArchBrick             拱曲面楔形砖（30°弧度，拼圆拱/圆柱基本单元）
//!   5. CurvedCylinderTrunk   歪曲圆柱 / 树干（锥度 + 顶点抖动 + 整体弯曲）
//!
//! 每个元素背后立一块 1.2×1.2 白色背板，便于肉眼对比颜色和尺度。
//! --demo 模式退出前会：
//!   - 打印每个元素的 NAME 和 SIZE
//!   - 做"类型常量 ↔ 实例 get_length / get_size / get_width..."API 一致性断言

use bevy::app::AppExit;
use bevy::ecs::message::MessageWriter;
use bevy::prelude::*;

use destr::common::{
    add_default_plugins, plain_material, spawn_camera_at, spawn_ground, spawn_sun,
};
use destr::demo::{request_exit, request_screenshot, DemoDriver};
use destr::elements::{
    ArchBrick, Brick, CementBlock, CurvedCylinderTrunk, DebrisPiece, Element, IrregularRock,
};

const CASE: &str = "basic_elements_showcase";
const ITEM_SPACING: f32 = 3.8;

fn main() {
    let demo = std::env::args().any(|a| a == "--demo");
    let mut app = App::new();
    add_default_plugins(&mut app, format!("destr · {CASE} — 基础元素陈列 × 6"));
    if demo {
        app.insert_resource(DemoDriver::default());
    }
    app.add_systems(Startup, setup);
    if demo {
        app.add_systems(Update, (demo_shot, demo_exit));
    }
    app.run();
}

struct Item { cn: &'static str }
const ITEMS: &[Item] = &[
    Item { cn: "标准砖" },
    Item { cn: "水泥块" },
    Item { cn: "碎砖(无边框)" },
    Item { cn: "不规则石块" },
    Item { cn: "拱曲面楔形砖" },
    Item { cn: "歪曲圆柱树干" },
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

    // 6 个元素跨度 (n-1)*3.8 ≈ 19 m → 镜头需要拉远并稍微抬高
    spawn_camera_at(
        &mut commands,
        Vec3::new(0.0, 4.4, 16.5),
        Vec3::new(0.0, 1.6, 0.0),
    );

    // 公共背板：1.3 × 1.3 × 0.1（树干很高，背板至少到 1.4 m）
    let backer_mesh = meshes.add(Cuboid::new(1.3, 1.3, 0.1).mesh().build());
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
            3 => spawn_one::<IrregularRock>(
                &mut commands, &mut meshes, &mut materials, &mut images,
                x, item.cn, backer_mesh.clone(), backer_mat.clone(),
            ),
            4 => spawn_one::<ArchBrick>(
                &mut commands, &mut meshes, &mut materials, &mut images,
                x, item.cn, backer_mesh.clone(), backer_mat.clone(),
            ),
            5 => spawn_trunk::<CurvedCylinderTrunk>(
                &mut commands, &mut meshes, &mut materials, &mut images,
                x, item.cn, backer_mesh.clone(), backer_mat.clone(),
            ),
            _ => unreachable!(),
        }
    }
}

/// spawn 立方体类元素（Brick / CementBlock / DebrisPiece / IrregularRock / ArchBrick）：
/// 本体中心 = (x, H/2, 0)，背板中心 = (x, 0.65, -0.7)
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
    let pos = Vec3::new(center_x, E::HEIGHT * 0.5, 0.0);
    let mesh_h = meshes.add(<E as Element>::base_mesh());
    let mat_h = <E as Element>::default_material(materials, images, Color::WHITE, 0.93);
    commands.spawn((
        Mesh3d(mesh_h),
        MeshMaterial3d(mat_h),
        Transform::from_translation(pos),
        Name::new(format!("element_{}", E::NAME)),
    ));
    commands.spawn((
        Mesh3d(backer_mesh),
        MeshMaterial3d(backer_mat),
        Transform::from_translation(Vec3::new(center_x, 0.65, -0.7)),
        Name::new(format!("backer_{}", E::NAME)),
    ));
}

/// spawn 树干（特殊：原点在 **底部中心**，高度从 0..CurvedCylinderTrunk::HEIGHT，
/// 所以 y 不用加一半偏移）。背板拉高到 1.7 m。
fn spawn_trunk<E: Element>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    center_x: f32,
    _cn: &'static str,
    backer_mesh: Handle<Mesh>,
    backer_mat: Handle<StandardMaterial>,
) {
    let pos = Vec3::new(center_x, 0.0, 0.0);
    let mesh_h = meshes.add(<E as Element>::base_mesh());
    let mat_h = <E as Element>::default_material(materials, images, Color::WHITE, 0.96);
    commands.spawn((
        Mesh3d(mesh_h),
        MeshMaterial3d(mat_h),
        Transform::from_translation(pos),
        Name::new(format!("element_{}", E::NAME)),
    ));
    commands.spawn((
        Mesh3d(backer_mesh),
        MeshMaterial3d(backer_mat),
        Transform::from_translation(Vec3::new(center_x, 1.7, -0.7)),
        Name::new(format!("backer_{}", E::NAME)),
    ));
}

// ── 演示模式 ───────────────────────────────────────────────────────

fn demo_shot(time: Res<Time>, mut demo: ResMut<DemoDriver>, mut commands: Commands) {
    if time.elapsed_secs() >= 0.8 && !demo.shot_taken {
        demo.shot_taken = true;
        destr::demo::shot_marker(CASE);
        request_screenshot(&mut commands, CASE);
    }
}

fn demo_exit(time: Res<Time>, mut exit: MessageWriter<AppExit>) {
    if time.elapsed_secs() > 2.5 {
        println!("\n=== [{CASE}] 演示结束：展示 {n} 个基础元素 ===", n = ITEMS.len());
        let list: [(&str, &str, Vec3); 6] = [
            ("标准砖",         Brick::NAME,                Brick::SIZE),
            ("水泥块",         CementBlock::NAME,          CementBlock::SIZE),
            ("碎砖(无边框)",   DebrisPiece::NAME,          DebrisPiece::SIZE),
            ("不规则石块",     IrregularRock::NAME,        IrregularRock::SIZE),
            ("拱曲面楔形砖",   ArchBrick::NAME,            ArchBrick::SIZE),
            ("歪曲圆柱树干",   CurvedCylinderTrunk::NAME,  CurvedCylinderTrunk::SIZE),
        ];
        for (cn, en, s) in list {
            println!("  · {:<14}  NAME={:<22}  SIZE=({:.2}, {:.2}, {:.2})", cn, en, s.x, s.y, s.z);
        }

        // 对所有元素各做一遍 API 一致性断言（只要有一个元素 API 错了，CI 直接失败）
        let all_pass = check_element::<Brick>()
            && check_element::<CementBlock>()
            && check_element::<DebrisPiece>()
            && check_element::<IrregularRock>()
            && check_element::<ArchBrick>()
            && check_element::<CurvedCylinderTrunk>();
        println!(
            "  · 6 类 Element API 一致性（SIZE↔get_length / get_length_xyz ↔ get_width/height/depth / get_size↔get_length）: {}",
            if all_pass { "✓ PASS" } else { "✗ FAIL" },
        );
        assert!(all_pass, "至少一个 Element 的 API 一致性失败");

        request_exit(&mut exit);
    }
}

fn check_element<E: Element>() -> bool {
    let z = E::default();
    let a = (z.get_length() - E::SIZE).length_squared() < 1e-6;
    let b = z.get_length_x() == E::WIDTH
        && z.get_length_y() == E::HEIGHT
        && z.get_length_z() == E::DEPTH;
    let c = z.get_width() == z.get_length_x()
        && z.get_height() == z.get_length_y()
        && z.get_depth() == z.get_length_z();
    let d = (z.get_size() - z.get_length()).length_squared() < 1e-6;
    a && b && c && d
}
