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
    add_default_plugins, flip_uv, plain_material, scale_uv, spawn_camera_at, spawn_ground,
    spawn_sun, tint, translate_uv,
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
            // ✅ 修改 1：锥度调小——顶径保持底径的 60%~75%，不再是 0.10 的"针头顶"
            //     现实中多数树干上下直径差只有 ~25~40%，这样视觉上更自然。
            let top_ratio = rng_range(seed, 30, 0.60, 0.75);
            let r1 = r0 * top_ratio;

            // ✅ 修改 2：弯曲度分三档（30% 直 / 50% 略歪 / 20% 明显弯）
            //           θ 扩成 -180°~180° 全方向，不再缺半边角度
            let bucket = rng2(seed, 40); // 0..1 区间决定档位
            let bend_mag = if bucket < 0.30 {
                rng_range(seed, 41, 0.0, 0.05)         // 直上直下：5cm 内近似直
            } else if bucket < 0.80 {
                rng_range(seed, 42, 0.08, 0.22)        // 略歪：8~22 cm
            } else {
                rng_range(seed, 43, 0.24, 0.52)        // 明显弯曲
            };
            let theta = rng_range(seed, 44,
                -180.0_f32.to_radians(), 180.0_f32.to_radians());   // 全 360°
            let bend = Vec2::new(theta.cos() * bend_mag, theta.sin() * bend_mag);

            // 放树干：mesh 按 PALETTE 三档棕褐色染色
            let mesh = curved_trunk_mesh(h, r0, r1, 24, 9, bend, seed); // 细分加一点更圆滑

            // ✅ 修改 3：花纹每棵独立可调 —— UV 随机：翻转 4 种 + 缩放 (0.65~1.6, 0.55~1.8) + 平移 (0~1 周)
            //           组合数 4 × 几十² × 几百 = 一眼看过去基本不重样
            let fx = rng2(seed, 71) < 0.5;
            let fy = rng2(seed, 72) < 0.5;
            let mesh = flip_uv(mesh, fx, fy);
            let su = rng_range(seed, 73, 0.65, 1.6);   // U 方向：围绕树干的"环向纹路密度"
            let sv = rng_range(seed, 74, 0.55, 1.8);   // V 方向：竖直方向"皴裂纹密度"
            let mesh = scale_uv(mesh, su, sv);
            let du = rng_range(seed, 75, 0.0, 1.0);    // 让每棵树的"第几圈纹路对齐"错开
            let dv = rng_range(seed, 76, 0.0, 1.0);    // 让每棵树的"第几米高度处的断层"错开
            let mesh = translate_uv(mesh, du, dv);

            // 颜色选三档（之前的逻辑保留）
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
        println!("  · 单棵树参数范围：");
        println!("    - 高 2.6~3.9 m，底径 0.56~0.88 m，顶径 = 底径 × 60%~75%（锥度明显调小）");
        println!("    - 弯曲度三档：约 30% 近似直（0~5cm），约 50% 略歪（8~22 cm），约 20% 明显弯（24~52 cm）");
        println!("    - 弯曲方向：360° 全向（不再集中半边）");
        println!("    - 花纹每棵独立：UV flip×4 / U缩放(0.65~1.6) / V缩放(0.55~1.8) / UV平移(各自错开)");
        request_exit(&mut exit);
    }
}
