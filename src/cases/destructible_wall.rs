//! Case 01: destructible_wall —— 可破坏砖墙（体素数据层 + chunk 网格 + 消息解耦）。
//!
//! 运行：
//!   cargo run --bin destructible_wall                 # 左键点墙打砖
//!   cargo run --bin destructible_wall -- --demo        # 自动打洞 + 截图 + 退出（CI/无人值守）
//!
//! 这个 case 是整个仓库最"教学向"的：它演示了 4 个以后每个 case 都会用到的架构原则：
//!   1. 数据与渲染分离（WallData 是一个纯 Resource）
//!   2. 变化局部化（chunk：破坏时只重建受影响的一块合并网格）
//!   3. 消息解耦（输入层只读、破坏层只写，Bevy 自动排执行顺序）
//!   4. 演示模式与真实输入走同一条事件管道（BlockPunched 消息）—— 验证的就是真实玩家路径

mod wall;

use bevy::app::AppExit;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use destr::common::{
    add_default_plugins, plain_material, spawn_default_camera, spawn_ground, spawn_sun,
};
use destr::demo::{request_exit, request_screenshot, DemoDriver};
use destr::elements::{half_after_scale, Brick, CementBlock, DebrisPiece, Element};

use self::wall::WallData;

// ── Case 名字常量（演示模式输出目录等用）────────────────────────────
const CASE: &str = "destructible_wall";

fn main() {
    let demo = std::env::args().any(|a| a == "--demo");
    let mut app = App::new();
    add_default_plugins(&mut app, format!("destr · {CASE} — 左键打砖"));

    // 数据层：一面墙就是一个 Resource
    app.insert_resource(WallData::new())
        .insert_resource(DemoDriver::default())
        .add_message::<BlockPunched>()
        .add_systems(Startup, setup)
        .add_systems(Update, (punch_on_click, apply_punch, debris_physics));

    if demo {
        app.add_systems(Update, (demo_punch, demo_shot, demo_exit));
    }
    app.run();
}

// ── 消息：这是"输入"和"执行"之间的唯一通道 ─────────────────────────
//
// 为什么用消息？MeshRayCast 系统内部占用 Res<Assets<Mesh>>（只读），
// 执行破坏又需要 ResMut<Assets<Mesh>>（读写）—— 同一系统里两者冲突。
// 拆成两个系统 + 一条消息，各拿各的访问权，Bevy 自动安排执行顺序。

#[derive(Message)]
struct BlockPunched {
    c: usize,
    y: usize,
    x: usize,
}

// ── 共享资源 ───────────────────────────────────────────────────────

/// 标记一块墙 chunk 实体：它负责表格里的哪片区域（查询用）。
#[derive(Component)]
struct WallChunk {
    cx: usize,
    cy: usize,
}

/// 所有材质句柄集中在这里：砖 / 水泥 / 碎砖（未来扩展直接加字段）。
/// 每种元素的 Mesh / 贴图 / 材质都走 Element trait 工厂，不用现场拼写参数。
#[derive(Resource)]
struct WallAssets {
    brick_material: Handle<StandardMaterial>,
    #[allow(dead_code)]
    cement_material: Handle<StandardMaterial>,
    debris_mesh: Handle<Mesh>,
}

// ── Startup: 场景拼装 ─────────────────────────────────────────────

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    wall: Res<WallData>,
) {
    // 公共：阳光、地面、相机
    spawn_sun(&mut commands);
    let plain = plain_material(&mut materials);
    spawn_ground(&mut commands, &mut meshes, plain.clone());
    spawn_default_camera(&mut commands);

    // ✅ 砖 / 水泥 / 碎砖：**全部用 Element trait 的 default_material 工厂**
    //    以后想调 roughness / 颜色只需改一个入参，不用再拼 StandardMaterial 字段。
    let brick_mat  = <Brick       as Element>::default_material(&mut materials, &mut images, Color::WHITE, 0.95);
    let cement_mat = <CementBlock as Element>::default_material(&mut materials, &mut images, Color::WHITE, 0.92);

    // ✅ 碎砖 mesh：走 DebrisPiece::base_mesh()，尺寸以后改 DebrisPiece::SIZE 全局生效。
    let debris_mesh = meshes.add(<DebrisPiece as Element>::base_mesh());

    commands.insert_resource(WallAssets {
        brick_material: brick_mat.clone(),
        cement_material: cement_mat,
        debris_mesh,
    });

    // 墙：每块 chunk 一个实体。3×2 = 6 个 draw call 撑起整面墙。
    for cy in 0..wall::chunks_y() {
        for cx in 0..wall::chunks_x() {
            let mesh = wall::build_chunk_mesh(&*wall, cx, cy);
            commands.spawn((
                WallChunk { cx, cy },
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(brick_mat.clone()),
                Transform::default(),
            ));
        }
    }
}

// ── 执行破坏（收到消息 → 改数据 → 重建 chunk → 生成碎砖）────────────

fn apply_punch(
    mut punches: MessageReader<BlockPunched>,
    mut wall: ResMut<WallData>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunks: Query<(Entity, &WallChunk, &mut Mesh3d)>,
    assets: Res<WallAssets>,
    mut commands: Commands,
) {
    for p in punches.read() {
        punch_block(&mut *wall, p.c, p.y, p.x, &mut meshes, &mut chunks, &assets, &mut commands);
    }
}

fn punch_block(
    wall: &mut WallData,
    c: usize,
    y: usize,
    x: usize,
    meshes: &mut Assets<Mesh>,
    chunks: &mut Query<(Entity, &WallChunk, &mut Mesh3d)>,
    assets: &WallAssets,
    commands: &mut Commands,
) {
    if !wall.alive(c, y, x) {
        return;
    }
    let pos = wall::block_center(c, y, x);
    wall.destroy(c, y, x);
    let (cx, cy) = wall::chunk_of(x, y);
    for (_, chunk, mut mesh3d) in chunks.iter_mut() {
        if chunk.cx == cx && chunk.cy == cy {
            **mesh3d = meshes.add(wall::build_chunk_mesh(wall, cx, cy));
            break;
        }
    }
    spawn_debris(pos, c, y, x, assets, commands);
}

// ── 鼠标点击：射线 → 命中 → 砖下标 → 发消息 ────────────────────────

fn punch_on_click(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    chunks: Query<Entity, With<WallChunk>>,
    mut ray_cast: MeshRayCast,
    mut punches: MessageWriter<BlockPunched>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let Ok((camera, cam_tf)) = cameras.single() else { return };
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else { return };
    let hits = ray_cast.cast_ray(ray, &MeshRayCastSettings::default());
    let Some((_, hit)) = hits.iter().find(|(e, _)| chunks.get(*e).is_ok()) else {
        return;
    };
    let inside = hit.point - hit.normal * 0.01;
    let Some((c, y, x)) = wall::block_from_point(inside) else { return };
    punches.write(BlockPunched { c, y, x });
}

// ── 碎砖物理（临时飞行实体 + 手写重力/弹跳/冻结）─────────────────────

#[derive(Component)]
struct Debris {
    velocity: Vec3,
    angvel: Vec3,
}

fn spawn_debris(
    pos: Vec3,
    c: usize,
    y: usize,
    x: usize,
    assets: &WallAssets,
    commands: &mut Commands,
) {
    use destr::tex::hash2;
    for i in 0..3 {
        let h1 = hash2(x as i32 * 7 + i, y as i32 * 13 + c as i32);
        let h2 = hash2(y as i32 * 5 + i, x as i32 * 11 + c as i32);
        let h3 = hash2(c as i32 * 17 + i, y as i32 * 3 + x as i32);
        commands.spawn((
            Mesh3d(assets.debris_mesh.clone()),
            MeshMaterial3d(assets.brick_material.clone()),
            Transform::from_translation(pos + Vec3::new((h1 - 0.5) * 0.5, (h2 - 0.5) * 0.3, (h3 - 0.5) * 0.4))
                .with_scale(Vec3::splat(0.6 + h1 * 0.8)),
            Debris {
                velocity: Vec3::new((h1 - 0.5) * 4.0, 2.5 + h2 * 3.0, 1.5 + h3 * 2.5),
                angvel: Vec3::new((h1 - 0.5) * 6.0, (h2 - 0.5) * 6.0, (h3 - 0.5) * 6.0),
            },
        ));
    }
}

fn debris_physics(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Debris)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    // 所有掉落物统一按 DebrisPiece 元素的 half-extent 算地面碰撞。
    // 以后改碎砖基准尺寸，只改 DebrisPiece 的常量即可，不用再手改这里的 0.22。
    let piece_size = DebrisPiece::SIZE;
    for (e, mut tf, mut d) in &mut q {
        d.velocity.y -= 15.0 * dt;
        tf.translation += d.velocity * dt;
        tf.rotate_x(d.angvel.x * dt);
        tf.rotate_y(d.angvel.y * dt);
        tf.rotate_z(d.angvel.z * dt);
        let half = half_after_scale(piece_size, tf.scale);
        let half_y = half.y;
        if tf.translation.y < half_y && d.velocity.y < 0.0 {
            tf.translation.y = half_y;
            if d.velocity.y < -3.0 {
                d.velocity.y = -d.velocity.y * 0.35;
                d.velocity.x *= 0.6;
                d.velocity.z *= 0.6;
                d.angvel *= 0.5;
            } else {
                commands.entity(e).remove::<Debris>();
            }
        }
        if tf.translation.length() > 60.0 {
            commands.entity(e).despawn();
        }
    }
}

// ── 演示模式（无人值守）：按时间表打孔 + 截图 + 退出 ─────────────────

const PUNCHES: [(usize, usize, usize); 8] = [
    (1, 5, 5), (1, 6, 5), (1, 5, 6), (1, 6, 6),
    (1, 4, 6), (1, 7, 5), (1, 4, 5), (1, 7, 6),
];

fn demo_punch(
    time: Res<Time>,
    mut demo: ResMut<DemoDriver>,
    mut punches: MessageWriter<BlockPunched>,
) {
    let t = time.elapsed_secs();
    let i = demo.actions_done;
    if i < PUNCHES.len() && t >= 0.3 + 0.35 * i as f32 {
        let (c, y, x) = PUNCHES[i];
        punches.write(BlockPunched { c, y, x });
        demo.actions_done += 1;
    }
}

fn demo_shot(time: Res<Time>, mut demo: ResMut<DemoDriver>, mut commands: Commands) {
    if time.elapsed_secs() >= 4.2 && !demo.shot_taken {
        demo.shot_taken = true;
        destr::demo::shot_marker(CASE);
        request_screenshot(&mut commands, CASE);
    }
}

fn demo_exit(
    time: Res<Time>,
    wall: Res<WallData>,
    mut exit: MessageWriter<AppExit>,
) {
    if time.elapsed_secs() > 5.0 {
        println!("\n=== [{CASE}] 演示结束：最终墙态 ===");
        wall::print_wall_section(&*wall, 1);
        request_exit(&mut exit);
    }
}
