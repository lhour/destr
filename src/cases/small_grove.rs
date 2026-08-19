//! Case 04: small_grove —— 小树林（歪曲树干 × N + 树根乱石）
//!
//! 树林布局：3 行 × 3 列（共 9 棵），在 XZ 平面按 3.0 m × 3.0 m 网格撒，XZ 各 ±10% 抖动避免太规整。
//! 每棵树参数：seed 不同，高度从 2.6~3.8 m、锥度从 0.30→0.12 到 0.42→0.14、
//!            bend 方向按一个"扇形角"摆（避免所有树都弯向同一方向）。
//! 每棵树脚下撒 2~5 块 IrregularRock 作为树根乱石，另外整体场景里零散撒 20 块 DebrisPiece 做表土碎物。

use bevy::app::AppExit;
use bevy::ecs::message::MessageWriter;
use bevy::prelude::*;

use destr::common::{
    add_default_plugins, plain_material, spawn_camera_at, spawn_ground, spawn_sun, tint,
};
use destr::demo::{request_exit, request_screenshot, DemoDriver};
use destr::elements::{
    curved_trunk_mesh, irregular_rock_mesh, CurvedCylinderTrunk, DebrisPiece, Element,
    IrregularRock,
};
use destr::tex::hash2;

const CASE: &str = "small_grove";
const ROWS: i32 = 3;
const COLS: i32 = 3;
const STEP_X: f32 = 3.0;
const STEP_Z: f32 = 3.0;

fn hex_to_lin4(hex: u32) -> [f32; 4] {
    let r = ((hex >> 16) & 0xff) as f32 / 255.0;
    let g = ((hex >> 8) & 0xff) as f32 / 255.0;
    let b = (hex & 0xff) as f32 / 255.0;
    fn f(u: f32) -> f32 {
        if u <= 0.040_45 { u / 12.92 } else { ((u + 0.055) / 1.055).powf(2.4) }
    }
    [f(r), f(g), f(b), 1.0]
}

fn main() {
    let demo = std::env::args().any(|a| a == "--demo");
    let mut app = App::new();
    add_default_plugins(&mut app, format!("destr · {CASE} — 小树林 ×9（树干 + 树根乱石）"));
    if demo {
        app.insert_resource(DemoDriver::default());
    }
    app.add_systems(Startup, setup);
    if demo {
        app.add_systems(Update, (demo_shot, demo_exit));
    }
    app.run();
}

// 确定性随机（不依赖外部 rand crate）
fn rng2(x: i32, y: i32) -> f32 { hash2(x, y) }
fn rng_range(x: i32, y: i32, lo: f32, hi: f32) -> f32 { lo + (hi - lo) * rng2(x, y) }

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    spawn_sun(&mut commands);
    let plain = plain_material(&mut materials);
    spawn_ground(&mut commands, &mut meshes, plain.clone());

    // 镜头稍微偏斜、稍拉高，避免所有树都正对镜头重叠；
    // 看 9 棵树 (±3m × ±3m 内)，目标点 (0, 1.2, 0)。
    spawn_camera_at(
        &mut commands,
        Vec3::new(-4.0, 4.6, 8.2),
        Vec3::new(0.0, 1.2, 0.0),
    );

    let trunk_mat = CurvedCylinderTrunk::default_material(&mut materials, &mut images, Color::WHITE, 0.96);
    let rock_mat = IrregularRock::default_material(&mut materials, &mut images, Color::WHITE, 0.94);
    let debris_mat = DebrisPiece::default_material(&mut materials, &mut images, Color::WHITE, 1.0);

    let mut tree_count = 0i32;
    let mut rock_count = 0i32;

    for row in 0..ROWS {
        for col in 0..COLS {
            let cx = -((COLS - 1) as f32) * 0.5 * STEP_X + (col as f32) * STEP_X;
            let cz = -((ROWS - 1) as f32) * 0.5 * STEP_Z + (row as f32) * STEP_Z;
            // XZ 上 ±30cm 抖动，避免笔直网格
            let jx = rng_range(col * 11, row * 31, -0.30, 0.30);
            let jz = rng_range(col * 17, row * 19, -0.30, 0.30);
            let tx = cx + jx;
            let tz = cz + jz;

            // 每棵树的独立参数：
            let seed = row * 100 + col;
            let h = rng_range(seed, 1, 2.6, 3.9);
            let r0 = rng_range(seed, 2, 0.28, 0.44);
            let r1 = rng_range(seed, 3, 0.10, 0.16);
            // bend 方向：θ 从 -40°..+160° 选一个方向，大小 0.15..0.45
            let theta = rng_range(seed, 4,
                -40.0_f32.to_radians(), 160.0_f32.to_radians());
            let bend_mag = rng_range(seed, 5, 0.15, 0.48);
            let bend = Vec2::new(theta.cos() * bend_mag, theta.sin() * bend_mag);

            // 放树干：mesh 按 PALETTE 三档棕褐色染色（否则 bark_texture 只有灰度，整体苍白）
            let mesh = curved_trunk_mesh(h, r0, r1, 18, 7, bend, seed);
            let pick = if rng2(seed, 60) < 0.18 {
                CurvedCylinderTrunk::PALETTE[2]
            } else if rng2(seed, 61) < 0.55 {
                CurvedCylinderTrunk::PALETTE[1]
            } else {
                CurvedCylinderTrunk::PALETTE[0]
            };
            let mesh = tint(mesh, hex_to_lin4(pick));
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(trunk_mat.clone()),
                Transform::from_translation(Vec3::new(tx, 0.0, tz)),
                Name::new(format!("tree_r{}_c{}", row, col)),
            ));
            tree_count += 1;

            // 树根乱石 2~5 块：放在树干底部 XZ，按 seed 决定数量
            let n_rocks = 2 + (rng2(seed, 77) * 4.0) as i32; // 2..=5
            for r in 0..n_rocks {
                let rx = tx + rng_range(seed, 1000 + r * 3, -r0 - 0.15, r0 + 0.15);
                let rz = tz + rng_range(seed, 1001 + r * 3, -r0 - 0.15, r0 + 0.15);
                let size = Vec3::new(
                    rng_range(seed, 2000 + r, 0.28, 0.55),
                    rng_range(seed, 2001 + r, 0.20, 0.45),
                    rng_range(seed, 2002 + r, 0.28, 0.55),
                );
                let ry = size.y * 0.5 - 0.05; // 石头稍微沉地（视觉上扎进土里，像长出来的根石）
                let m = irregular_rock_mesh(size, seed + r * 7);
                let m = tint(m, hex_to_lin4(IrregularRock::PALETTE[(r as usize) % 3]));
                // 随机旋转（绕 Y）
                let yaw = rng_range(seed, 3000 + r, 0.0, std::f32::consts::TAU);
                commands.spawn((
                    Mesh3d(meshes.add(m)),
                    MeshMaterial3d(rock_mat.clone()),
                    Transform {
                        translation: Vec3::new(rx, ry, rz),
                        rotation: Quat::from_rotation_y(yaw),
                        ..default()
                    },
                    Name::new(format!("root_r{}_c{}_i{}", row, col, r)),
                ));
                rock_count += 1;
            }
        }
    }

    // 额外撒 22 块 DebrisPiece 做地表碎物（整个 XZ：±6.0 × ±6.0）
    for i in 0..22i32 {
        let dx = rng_range(i, 99, -6.0, 6.0);
        let dz = rng_range(i, 98, -6.0, 6.0);
        let scale = Vec3::splat(rng_range(i, 97, 0.8, 1.4));
        let ry = DebrisPiece::HEIGHT * 0.5 * scale.y - 0.01;
        let yaw = rng_range(i, 96, 0.0, std::f32::consts::TAU);
        let m = DebrisPiece::base_mesh();
        let m = tint(m, hex_to_lin4(DebrisPiece::PALETTE[(i as usize) % 3]));
        commands.spawn((
            Mesh3d(meshes.add(m)),
            MeshMaterial3d(debris_mat.clone()),
            Transform {
                translation: Vec3::new(dx, ry, dz),
                rotation: Quat::from_rotation_y(yaw),
                scale,
            },
            Name::new(format!("litter_{i:02}")),
        ));
    }

    // 保存到资源里方便 demo_exit 打印（直接写成常量也可以，这里保留计数）
    commands.insert_resource(GroveStats {
        trees: tree_count,
        rocks: rock_count,
        litters: 22,
    });
}

#[derive(Resource)]
struct GroveStats { trees: i32, rocks: i32, litters: i32 }

// ── 演示模式 ───────────────────────────────────────────────────────

fn demo_shot(time: Res<Time>, mut demo: ResMut<DemoDriver>, mut commands: Commands) {
    if time.elapsed_secs() >= 1.2 && !demo.shot_taken {
        demo.shot_taken = true;
        destr::demo::shot_marker(CASE);
        request_screenshot(&mut commands, CASE);
    }
}

fn demo_exit(time: Res<Time>, stats: Res<GroveStats>, mut exit: MessageWriter<AppExit>) {
    if time.elapsed_secs() > 2.8 {
        println!("\n=== [{CASE}] 小树林统计 ===");
        println!("  · 网格布局：{} 行 × {} 列，XZ 抖动 ±0.30 m，共 树 {} 棵",
                 ROWS, COLS, stats.trees);
        println!("  · 树根乱石：共 {} 块 IrregularRock（每棵树下 2~5 块，随机尺寸/旋转/颜色）",
                 stats.rocks);
        println!("  · 地表碎物：共 {} 块 DebrisPiece（整个场景 12×12 m 内随机撒）", stats.litters);
        println!("  · 单棵树参数范围：高 2.6~3.9 m，底径 0.56~0.88 m，顶径 0.20~0.32 m，弯曲量 0.15~0.48 m");
        request_exit(&mut exit);
    }
}
