//! Case 03: semicircular_arch —— 半圆拱门。
//!
//! 组成：
//!   - 拱圈：12 块 ArchBrick（每块 15°）绕 +Y 轴旋转 k·15°，拼一个内径 1.6 / 外径 2.0 的半圆拱。
//!   - 支座（左右墩）：各 2 列 × 4 皮 Brick，错缝砌法（和 destructible_wall 相同灰缝逻辑）。
//!   - 基础：左右各一条 CementBlock 墩基，中间一个 CementBlock 台阶/槛。
//!
//! 镜头正对 +Z 前方拍整个拱门，高度 1.4~2.0，足够看到支座和拱圈接缝。
//! --demo：1.2s 截图 → 2.5s 打印用了哪些元素 + 退出。

use bevy::app::AppExit;
use bevy::ecs::message::MessageWriter;
use bevy::math::DVec2;
use bevy::prelude::*;

use destr::common::{
    add_default_plugins, flip_uv, plain_material, spawn_camera_at, spawn_ground, spawn_sun, tint,
};
use destr::demo::{request_exit, request_screenshot, DemoDriver};
use destr::elements::{arch_brick_mesh, ArchBrick, Brick, CementBlock, Element};
use destr::tex::hash2;

const CASE: &str = "semicircular_arch";
const ARCH_BRICKS: i32 = 12;           // 12 × 15° = 180° 半圆
const PIER_COLS: i32 = 2;              // 支座每侧 2 列砖
const PIER_ROWS: i32 = 4;              // 支座每侧 4 皮砖
const BASE_BW: f32 = 1.30;             // 墩基宽（X 向）
const BASE_BD: f32 = 0.85;             // 墩基厚（Z 向）
const BASE_BH: f32 = 0.30;             // 墩基高

// 用 ArchBrick 自带常量拼一块"自定义弧度"的楔形砖（避免主 setup 散落细节）。
// 这两个 helper 是 case 本地函数（不是 ArchBrick 的 impl 项）——E0116 禁止在 bin crate 里给外部类型添加固有 impl。
fn ab_custom(arc_rad: f32) -> Mesh {
    arch_brick_mesh(
        ArchBrick::R_OUTER, ArchBrick::THICK, arc_rad,
        ArchBrick::HEIGHT, 4,
    )
}
fn ab_r_inner() -> f32 { ArchBrick::R_OUTER - ArchBrick::THICK }

fn main() {
    let demo = std::env::args().any(|a| a == "--demo");
    let mut app = App::new();
    add_default_plugins(&mut app, format!("destr · {CASE} — 半圆拱门（拱圈 ×12 ArchBrick）"));
    if demo {
        app.insert_resource(DemoDriver::default());
    }
    app.add_systems(Startup, setup);
    if demo {
        app.add_systems(Update, (demo_shot, demo_exit));
    }
    app.run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    spawn_sun(&mut commands);
    let plain = plain_material(&mut materials);
    spawn_ground(&mut commands, &mut meshes, plain.clone());

    // 镜头：正前方 +Z 方向稍高，整张拱门居中
    spawn_camera_at(
        &mut commands,
        Vec3::new(0.0, 2.3, 5.6),
        Vec3::new(0.0, 1.25, 0.0),
    );

    // ---- 材质：共用 Brick / CementBlock 默认材质（走 Element API） ----
    let arch_mat = ArchBrick::default_material(&mut materials, &mut images, Color::WHITE, 0.92);
    let brick_mat = Brick::default_material(&mut materials, &mut images, Color::WHITE, 0.93);
    let cement_mat = CementBlock::default_material(&mut materials, &mut images, Color::WHITE, 0.95);

    // ---- 先放基础（左右墩基 + 中间门槛） ----
    let foundation_h = BASE_BH;
    // 拱圈内半径 = R_OUTER - THICK = 1.6，支座内边紧贴拱圈起点 ±1.6
    // 支座每侧 2 列 Brick（宽 2.0），半宽 1.0 → 支座中心 X = ±(1.6 + 1.0) = ±2.6
    let pier_half_w = Brick::WIDTH * (PIER_COLS as f32) * 0.5;
    let pier_center_x = ab_r_inner() + pier_half_w;
    // 拱圈每块 ArchBrick 用的尺寸：R_OUTER=2.0 R_INNER=1.6 ARC=π/12；这里按相同尺寸实例化（通过 arch_brick_one()）

    // 基础：比支座宽 0.3 m 每侧
    let base_half_w = pier_half_w + 0.30;
    let base_d = BASE_BD;
    for side in [-1i32, 1] {
        let cx = (side as f32) * pier_center_x;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(base_half_w * 2.0, foundation_h, base_d).mesh().build())),
            MeshMaterial3d(cement_mat.clone()),
            Transform::from_translation(Vec3::new(cx, foundation_h * 0.5, 0.0)),
            Name::new(format!("base_{}", if side < 0 { "L" } else { "R" })),
        ));
    }
    // 中间台阶/门槛：两墩之间的空间，宽 = 2·pier_center_x - 2·pier_half_w，厚 0.7，高 0.15
    let sill_w = pier_center_x * 2.0 - pier_half_w * 2.0 - 0.12;
    let sill_h = 0.15;
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(sill_w, sill_h, 0.70).mesh().build())),
        MeshMaterial3d(cement_mat.clone()),
        Transform::from_translation(Vec3::new(0.0, foundation_h + sill_h * 0.5, 0.0)),
        Name::new("sill"),
    ));

    // ---- 左右砖砌支座（每侧 PIER_COLS×PIER_ROWS，错缝砌） ----
    let y_start = foundation_h;
    // Brick::painted_mesh 内部已经"选色 + flip_uv + tint"，并且 common::tint 的签名是
    // `tint(m: Mesh, c: [f32; 4]) -> Mesh`（不能再额外传第 3 个参数），
    // 所以这里直接用 painted_mesh，不再做二次 tint。
    for side in [-1i32, 1] {
        for y in 0..PIER_ROWS {
            let offset = if y % 2 == 0 { 0.0 } else { Brick::WIDTH * 0.5 };
            let center_x = (side as f32) * pier_center_x;
            for col in 0..PIER_COLS {
                let col_idx = col as f32;
                let bx = center_x
                    + (col_idx - (PIER_COLS - 1) as f32 * 0.5) * Brick::WIDTH
                    + offset;
                let by = y_start + (y as f32) * Brick::HEIGHT + Brick::HEIGHT * 0.5;
                let pick = (y * 3 + col * 5 + side.unsigned_abs() as i32 * 7) as i32;
                let m = <Brick as Element>::painted_mesh(
                    col, y, 0, pick, Vec3::ZERO, true,
                );
                commands.spawn((
                    Mesh3d(meshes.add(m)),
                    MeshMaterial3d(brick_mat.clone()),
                    Transform::from_translation(Vec3::new(bx, by, 0.0)),
                    Name::new(format!("pier_{}_y{}_c{}", if side<0{"L"}else{"R"}, y, col)),
                ));
            }
        }
    }

    // ---- 拱圈：12 块 ArchBrick 每块 15°，旋转 k·15° ----
    // 拼法：每块弧度 one_arc = π/12（15°），绕 +Y 轴旋转 θ_k = -π/2 + (k+0.5)·one_arc。
    // 圆心放在支座顶上方 (0, base + 4皮砖, 0) — 砖自己的 (R_OUTER=2.0, R_INNER=1.6)
    // 正好让拱脚（θ=±90°处的内弧最低点）贴紧支座最内侧一列砖的顶部。
    let one_arc = std::f32::consts::PI / (ARCH_BRICKS as f32); // 15°
    let center_y = y_start + (PIER_ROWS as f32) * Brick::HEIGHT;

    for k in 0..ARCH_BRICKS {
        // 先把楔形砖（默认弯曲在 X-Z 平面）绕 +Y 旋到段角，再绕 +X 旋 -90° 把"XZ 弯转"转到"XY 弯转"：
        //   结果：点 (r sinθ, 0, r cosθ) 经过 (R_y(yaw) · R_x(-π/2)) 后变成 (r sin(θ+yaw), r cos(θ+yaw), 0)
        //   ——圆心角 θ+yaw = -π/2 → 左拱脚 (-R, 0, 0)，0 → 顶部 (0, R, 0)，+π/2 → 右拱脚 (R, 0, 0)，
        //     正好是"人能穿过去的正面门形拱"。
        let yaw = -std::f32::consts::FRAC_PI_2 + (k as f32 + 0.5) * one_arc;
        let mut t = Transform::from_translation(Vec3::new(0.0, center_y, 0.0));
        t.rotate_y(yaw);
        t.rotate_x(-std::f32::consts::FRAC_PI_2);

        // 每块砖"不同 UV 镜像 + 三档选色"，让接缝处看起来不重复。
        let m = ab_custom(one_arc);
        let fx = hash2(k, 11) < 0.5;
        let fy = hash2(k, 13) < 0.5;
        let m = flip_uv(m, fx, fy);
        // 调色板采样：把 hex → 线性 srgb [f32;4]
        let pal = <ArchBrick as Element>::PALETTE;
        let pick = if hash2(k, 17) < 0.18 {
            pal[2]
        } else if hash2(k, 19) < 0.55 {
            pal[1]
        } else {
            pal[0]
        };
        let col = hex_to_lin4(pick);
        let m = tint(m, col);

        commands.spawn((
            Mesh3d(meshes.add(m)),
            MeshMaterial3d(arch_mat.clone()),
            t,
            Name::new(format!("arch_ring_brick_{k:02}")),
        ));
    }
}

// Case 本地辅助：ArchBrick 的 hex 调色板 → 线性 [f32;4]
fn hex_to_lin4(hex: u32) -> [f32; 4] {
    let r = ((hex >> 16) & 0xff) as f32 / 255.0;
    let g = ((hex >> 8) & 0xff) as f32 / 255.0;
    let b = (hex & 0xff) as f32 / 255.0;
    // Srgb → Linear（近似，够用）
    fn f(u: f32) -> f32 {
        if u <= 0.040_45 { u / 12.92 } else { ((u + 0.055) / 1.055).powf(2.4) }
    }
    [f(r), f(g), f(b), 1.0]
}

// 把没用到的导入压掉，防止死代码警告。
#[allow(dead_code)]
fn _touch(_: DVec2) {}

// ── 演示模式 ───────────────────────────────────────────────────────

fn demo_shot(time: Res<Time>, mut demo: ResMut<DemoDriver>, mut commands: Commands) {
    if time.elapsed_secs() >= 1.2 && !demo.shot_taken {
        demo.shot_taken = true;
        destr::demo::shot_marker(CASE);
        request_screenshot(&mut commands, CASE);
    }
}

fn demo_exit(time: Res<Time>, mut exit: MessageWriter<AppExit>) {
    if time.elapsed_secs() > 2.6 {
        println!("\n=== [{CASE}] 半圆拱门构造 ===");
        println!("  · 支座：左右各 {}×{} Brick（错缝），基础 CementBlock 3 条（2 墩基 + 1 门槛）",
                 PIER_COLS, PIER_ROWS);
        println!("  · 拱圈：{} 块 ArchBrick × 每块 {:.1}°，内/外半径 {:.2}/{:.2} m，拼半圆 180°",
                 ARCH_BRICKS,
                 180.0 / ARCH_BRICKS as f32,
                 ab_r_inner(), ArchBrick::R_OUTER);
        println!("  · 拱圈底部 y = 基础 {:.2} + 支座 {:.2} = {:.2} m",
                 BASE_BH, PIER_ROWS as f32 * Brick::HEIGHT,
                 BASE_BH + PIER_ROWS as f32 * Brick::HEIGHT);
        println!("  · 拱顶 (X=0, {:.2}, 0) ~ 外半径 {:.2} 距离相机 {:.2} m",
                 BASE_BH + PIER_ROWS as f32 * Brick::HEIGHT + ArchBrick::R_OUTER,
                 ArchBrick::R_OUTER,
                 Vec3::new(0.0, 2.3, 5.6).distance(Vec3::new(0.0, 1.25, 0.0)));
        request_exit(&mut exit);
    }
}
