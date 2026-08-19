//! Case 01: destructible_wall —— 可破坏砖墙（体素数据层 + chunk 网格 + 消息解耦 + 冲击破坏）。
//!
//! 运行：
//!   cargo run --bin destructible_wall                 # 左键扔石头（撞墙→砖碎），右键手敲单块砖
//!   cargo run --bin destructible_wall -- --demo        # 自动扔 3 块石头打洞 + 截图 + 退出
//!
//! 冲击破坏机制（核心新增）：
//!   · 每个实现 Element trait 的类型都有 IMPACT_RESISTANCE（抗冲击）和 MASS（质量）
//!   · 投掷物 Projectile 组件自带 velocity + mass
//!   · 每帧做"投掷物 AABB ∩ 墙 Brick AABB"检测，命中时算动能 0.5·m·v²
//!   · 动能 > 砖块 IMPACT_RESISTANCE 就碎；碎砖飞溅速度按撞击方向 × √(剩余能量) 赋
//!   · 投掷物撞后按"撞墙系数"反弹，石头碰墙自己不碎（抗冲击高）

mod wall;

use bevy::app::AppExit;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use destr::common::{
    add_default_plugins, plain_material, spawn_default_camera, spawn_ground, spawn_sun, tint,
};
use destr::demo::{request_exit, request_screenshot, DemoDriver};
use destr::elements::{
    half_after_scale, Brick, CementBlock, CurvedCylinderTrunk, DebrisPiece, Element, IrregularRock,
};
use destr::tex::hash2;

use self::wall::WallData;

// ── Case 名字常量 ─────────────────────────────────────────────────
const CASE: &str = "destructible_wall";

fn main() {
    let demo = std::env::args().any(|a| a == "--demo");
    let mut app = App::new();
    add_default_plugins(&mut app, format!("destr · {CASE} — 左键扔石头 / 右键手敲"));

    app.insert_resource(WallData::new())
        .insert_resource(DemoDriver::default())
        .add_message::<BlockPunched>()
        .add_message::<ThrowRock>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                throw_on_click,            // 左键扔石头 → 发 ThrowRock 消息
                apply_throw,               // 收到 ThrowRock → 生成 Projectile
                punch_on_rightclick,       // 右键手敲（直接发 BlockPunched，老逻辑）
                apply_punch,               // BlockPunched → 改墙数据 + spawn 碎砖
                projectile_physics,        // 投掷物：重力 + 位移 + 碰墙检测 + 冲击破坏
                debris_physics,            // 碎砖：重力 + 地面反弹 + AABB 互碰
            ),
        );

    if demo {
        app.add_systems(Update, (demo_throw, demo_shot, demo_exit));
    }
    app.run();
}

// ── 消息 ──────────────────────────────────────────────────────────

#[derive(Message)]
struct BlockPunched {
    c: usize,
    y: usize,
    x: usize,
}

/// 扔一块石头：从相机位置朝屏幕点发射。
#[derive(Message)]
struct ThrowRock {
    origin: Vec3,
    direction: Vec3,   // 单位向量
    speed: f32,        // m/s
    seed: i32,         // 决定石头形状和颜色
}

// ── 共享资源 / 组件 ───────────────────────────────────────────────

#[derive(Component)]
struct WallChunk {
    cx: usize,
    cy: usize,
}

#[derive(Resource)]
struct WallAssets {
    brick_material: Handle<StandardMaterial>,
    #[allow(dead_code)]
    cement_material: Handle<StandardMaterial>,
    debris_mesh: Handle<Mesh>,
    rock_material: Handle<StandardMaterial>,
}

/// 投掷物（目前就是扔出去的 IrregularRock）——带速度、质量、半径，每帧做墙 AABB 检测。
#[derive(Component)]
struct Projectile {
    velocity: Vec3,
    angvel: Vec3,
    mass: f32,
    /// AABB half-extent（按 IrregularRock::SIZE * scale 算）
    half: Vec3,
}

/// 碎砖：飞行中的破坏产物，重力 + 碰地反弹 + 互相 AABB 碰。
#[derive(Component)]
struct Debris {
    velocity: Vec3,
    angvel: Vec3,
    half: Vec3,
    mass: f32,
}

// ── Startup: 场景拼装 ─────────────────────────────────────────────

fn lin4_from_hex(hex: u32) -> [f32; 4] {
    let r = ((hex >> 16) & 0xff) as f32 / 255.0;
    let g = ((hex >> 8) & 0xff) as f32 / 255.0;
    let b = (hex & 0xff) as f32 / 255.0;
    fn f(u: f32) -> f32 {
        if u <= 0.040_45 { u / 12.92 } else { ((u + 0.055) / 1.055).powf(2.4) }
    }
    [f(r), f(g), f(b), 1.0]
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    wall: Res<WallData>,
) {
    spawn_sun(&mut commands);
    let plain = plain_material(&mut materials);
    spawn_ground(&mut commands, &mut meshes, plain.clone());
    spawn_default_camera(&mut commands);

    let brick_mat  = <Brick       as Element>::default_material(&mut materials, &mut images, Color::WHITE, 0.95);
    let cement_mat = <CementBlock as Element>::default_material(&mut materials, &mut images, Color::WHITE, 0.92);
    let rock_mat   = <IrregularRock as Element>::default_material(&mut materials, &mut images, Color::WHITE, 0.93);
    let debris_mesh = meshes.add(<DebrisPiece as Element>::base_mesh());

    commands.insert_resource(WallAssets {
        brick_material: brick_mat.clone(),
        cement_material: cement_mat,
        debris_mesh,
        rock_material: rock_mat,
    });

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

// ── 扔石头：左键 → 射线 → ThrowRock 消息 ──────────────────────────

fn throw_on_click(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut throws: MessageWriter<ThrowRock>,
    mut counter: Local<i32>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let Ok((camera, cam_tf)) = cameras.single() else { return };

    // 取"屏幕点对应的世界射线"——方向就是投掷方向
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else { return };
    let origin = ray.origin;
    let dir = ray.direction.normalize_or_zero();
    if dir == Vec3::ZERO { return; }

    *counter += 1;
    throws.write(ThrowRock {
        origin,
        direction: dir,
        speed: 25.0,        // 25 m/s ≈ 职业棒球投球速度，能轻松砸碎砖
        seed: *counter,
    });
}

// ── 右键手敲（老逻辑保留，方便对比——直接命中 wall chunk 发 BlockPunched）

fn punch_on_rightclick(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    chunks: Query<Entity, With<WallChunk>>,
    mut ray_cast: MeshRayCast,
    mut punches: MessageWriter<BlockPunched>,
) {
    if !buttons.just_pressed(MouseButton::Right) {
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

// ── 生成投掷物实体 ─────────────────────────────────────────────────

fn apply_throw(
    mut throws: MessageReader<ThrowRock>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    assets: Res<WallAssets>,
) {
    for t in throws.read() {
        // 石头：默认 IrregularRock::SIZE 再缩成 0.55，手感更像一块"能扔的石头"
        let scale = 0.55;
        let size = IrregularRock::SIZE * scale;
        let mesh = destr::elements::irregular_rock_mesh(size, t.seed);
        // 三档颜色，保证每块不一样
        let col = IrregularRock::PALETTE[((hash2(t.seed, 9) * 3.0) as usize).min(2)];
        let mesh = tint(mesh, lin4_from_hex(col));

        // 质量 = 元素质量 × 缩放线性（体感值，不按体积 scale³ 真算——保证砸得动砖）
        let mass = IrregularRock::MASS * scale;
        let half = Vec3::new(size.x, size.y, size.z) * 0.5;

        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(assets.rock_material.clone()),
            Transform::from_translation(t.origin),
            Projectile {
                velocity: t.direction * t.speed,
                angvel: Vec3::new(
                    (hash2(t.seed, 1) - 0.5) * 12.0,
                    (hash2(t.seed, 2) - 0.5) * 12.0,
                    (hash2(t.seed, 3) - 0.5) * 12.0,
                ),
                mass,
                half,
            },
            Name::new(format!("rock_{}", t.seed)),
        ));
    }
}

// ── 执行"手敲破坏"（老逻辑复用）─────────────────────────────────────

fn apply_punch(
    mut punches: MessageReader<BlockPunched>,
    mut wall: ResMut<WallData>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunks: Query<(Entity, &WallChunk, &mut Mesh3d)>,
    assets: Res<WallAssets>,
    mut commands: Commands,
) {
    for p in punches.read() {
        punch_block(&mut *wall, p.c, p.y, p.x, &mut meshes, &mut chunks, &assets, &mut commands, None);
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
    splash_dir: Option<Vec3>,  // 碎砖飞溅偏好方向：None=伪随机乱喷，Some=偏向撞击方向
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
    spawn_debris(pos, c, y, x, assets, commands, splash_dir);
}

// ── 碎砖生成（可带撞击方向）───────────────────────────────────────

fn spawn_debris(
    pos: Vec3,
    c: usize,
    y: usize,
    x: usize,
    assets: &WallAssets,
    commands: &mut Commands,
    splash_dir: Option<Vec3>,
) {
    for i in 0..3 {
        let h1 = hash2(x as i32 * 7 + i, y as i32 * 13 + c as i32);
        let h2 = hash2(y as i32 * 5 + i, x as i32 * 11 + c as i32);
        let h3 = hash2(c as i32 * 17 + i, y as i32 * 3 + x as i32);
        let scale = 0.6 + h1 * 0.8;

        // 速度：如果有 splash_dir，就在"撞击方向 + 随机扰动"的基础上给能量，
        //       否则沿用原来的伪随机（手敲、演示模式走这里）。
        let base_random = Vec3::new((h1 - 0.5) * 4.0, 2.5 + h2 * 3.0, 1.5 + h3 * 2.5);
        let velocity = match splash_dir {
            Some(dir) => {
                // 撞击方向为正前方，再叠 40% 随机，保证看起来不整齐
                let dir = dir.normalize_or(Vec3::Z);
                let speed = 3.0 + h2 * 5.0;   // 3~8 m/s，能量来自被击碎的动能
                dir * speed + base_random * 0.4
            }
            None => base_random,
        };

        let half = half_after_scale(DebrisPiece::SIZE, Vec3::splat(scale));
        commands.spawn((
            Mesh3d(assets.debris_mesh.clone()),
            MeshMaterial3d(assets.brick_material.clone()),
            Transform::from_translation(pos + Vec3::new((h1 - 0.5) * 0.5, (h2 - 0.5) * 0.3, (h3 - 0.5) * 0.4))
                .with_scale(Vec3::splat(scale)),
            Debris {
                velocity,
                angvel: Vec3::new((h1 - 0.5) * 6.0, (h2 - 0.5) * 6.0, (h3 - 0.5) * 6.0),
                half,
                mass: DebrisPiece::MASS * scale.powi(3),
            },
        ));
    }
}

// ── 投掷物物理 + 冲击破坏（核心）───────────────────────────────────

fn projectile_physics(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Projectile)>,
    mut wall: ResMut<WallData>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunks: Query<(Entity, &WallChunk, &mut Mesh3d)>,
    assets: Res<WallAssets>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let g = 18.0;   // 石头比碎砖稍重，手感更好

    // 收集本帧命中的砖块（避免边 iter 边改）
    //   每项 = (投掷物实体, c, y, x, 撞击位置, 撞击法向(从投掷物指向砖外), 剩余速度反射方向)
    // 用"子步长"做穿墙保险：每帧拆成 SUB 小步，防止高速石头直接穿墙
    const SUB: usize = 3;
    let sub_dt = dt / SUB as f32;

    let mut pending_despawn: Vec<Entity> = Vec::new();

    for (e, mut tf, mut p) in &mut q {
        for _ in 0..SUB {
            // 1) 子步积分
            p.velocity.y -= g * sub_dt;
            let delta = p.velocity * sub_dt;
            tf.translation += delta;
            tf.rotate_x(p.angvel.x * sub_dt);
            tf.rotate_y(p.angvel.y * sub_dt);
            tf.rotate_z(p.angvel.z * sub_dt);

            // 2) 地面碰撞（简单反弹）
            if tf.translation.y - p.half.y < 0.0 {
                tf.translation.y = p.half.y;
                if p.velocity.y < -2.0 {
                    p.velocity.y = -p.velocity.y * 0.40;
                    p.velocity.x *= 0.55;
                    p.velocity.z *= 0.55;
                    p.angvel *= 0.6;
                } else {
                    // 速度太小：石头落地变成场景的一部分（移除 Projectile）
                    commands.entity(e).remove::<Projectile>();
                    break;
                }
            }

            // 3) 墙 AABB 重叠检测：取投掷物 AABB 覆盖的所有砖块坐标，逐个 check alive
            let _aabb_min = tf.translation - p.half;
            let _aabb_max = tf.translation + p.half;
            let mut hit_info: Option<(usize, usize, usize, Vec3, Vec3)> = None;
            // 扫一块覆盖范围（取 4 个角点 + 中心 → 找砖下标 → 去重）
            let mut probes: Vec<Vec3> = Vec::with_capacity(9);
            probes.push(tf.translation);
            for &(sx, sy, sz) in &[
                (-1.0_f32, -1.0, -1.0), ( 1.0, -1.0, -1.0), (-1.0,  1.0, -1.0), ( 1.0,  1.0, -1.0),
                (-1.0, -1.0,  1.0), ( 1.0, -1.0,  1.0), (-1.0,  1.0,  1.0), ( 1.0,  1.0,  1.0),
            ] {
                probes.push(tf.translation + Vec3::new(p.half.x * sx, p.half.y * sy, p.half.z * sz));
            }
            for probe in &probes {
                let Some((c, y, x)) = wall::block_from_point(*probe) else { continue };
                if !wall.alive(c, y, x) { continue; }
                // 算砖中心 → 判定命中面法向（投掷物相对砖的位移，取最大绝对值分量）
                let bc = wall::block_center(c, y, x);
                let to_brick = bc - tf.translation;
                let mut normal = Vec3::ZERO;
                if to_brick.x.abs() > to_brick.y.abs() && to_brick.x.abs() > to_brick.z.abs() {
                    normal.x = to_brick.x.signum();
                } else if to_brick.y.abs() > to_brick.z.abs() {
                    normal.y = to_brick.y.signum();
                } else {
                    normal.z = to_brick.z.signum();
                }
                if normal == Vec3::ZERO { normal = -p.velocity.normalize_or(Vec3::Z); }
                hit_info = Some((c, y, x, tf.translation, normal));
                break;
            }

            if let Some((c, y, x, _hit_pos, hit_normal)) = hit_info {
                // 4) 冲击动能判定
                let speed_sq = p.velocity.length_squared();
                let kinetic = 0.5 * p.mass * speed_sq;

                if kinetic >= Brick::IMPACT_RESISTANCE {
                    // ✅ 撞碎！——把这块砖打掉，碎砖飞溅方向沿撞击方向（-hit_normal 反推入射）
                    let incident_dir = -hit_normal;
                    punch_block(&mut *wall, c, y, x, &mut meshes, &mut chunks, &assets, &mut commands, Some(incident_dir));

                    // 碎砖破坏时还会"向相邻砖扩散"一点：动能有富余的话，把周围 6 邻居也尝试碎一块
                    let excess = kinetic - Brick::IMPACT_RESISTANCE;
                    if excess > Brick::IMPACT_RESISTANCE * 0.8 {
                        for (dc, dy, dx) in [
                            (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1), (1, 0, 0), (-1_i32, 0, 0),
                        ] {
                            if excess < Brick::IMPACT_RESISTANCE { break; }
                            let (nc, ny, nx) = (c as i32 + dc, y as i32 + dy, x as i32 + dx);
                            if nc < 0 || ny < 0 || nx < 0 { continue; }
                            punch_block(&mut *wall, nc as usize, ny as usize, nx as usize,
                                &mut meshes, &mut chunks, &assets, &mut commands, Some(incident_dir));
                            // 注意：punch_block 内部已处理 alive=false，所以重复调用安全
                        }
                    }

                    // 投掷物自己：反弹（反射 velocity 按法向），速度衰减
                    let vn = p.velocity.dot(hit_normal);
                    if vn < 0.0 {
                        p.velocity -= (1.0 + 0.25) * vn * hit_normal;   // 反弹系数 0.25
                    }
                    p.velocity *= 0.45;   // 撞砖损失一大半能量
                    p.angvel *= 0.5;
                    // 把位置从砖块里推出来：沿 hit_normal 推一个 half 距离
                    tf.translation += hit_normal * (p.half.x.max(p.half.y).max(p.half.z));
                } else {
                    // ❌ 没碎：投掷物反弹（撞击面反射）
                    let vn = p.velocity.dot(hit_normal);
                    if vn < 0.0 {
                        p.velocity -= (1.0 + 0.55) * vn * hit_normal;   // 反弹系数 0.55
                    }
                    p.velocity *= 0.7;
                    p.angvel *= 0.6;
                    // 推离砖面
                    tf.translation += hit_normal * (p.half.x.max(p.half.y).max(p.half.z) * 0.6);
                }
            }

            // 5) 超出世界：回收
            if tf.translation.length() > 80.0 {
                pending_despawn.push(e);
                break;
            }
        }
    }
    for e in pending_despawn {
        commands.entity(e).despawn();
    }
}

// ── 碎砖物理 + AABB 互碰（地面碰撞 + 互相重叠弹开）─────────────────

fn debris_physics(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Debris)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let g = 15.0;

    for (e, mut tf, mut d) in &mut q {
        // 重力 + 位移 + 旋转
        d.velocity.y -= g * dt;
        tf.translation += d.velocity * dt;
        tf.rotate_x(d.angvel.x * dt);
        tf.rotate_y(d.angvel.y * dt);
        tf.rotate_z(d.angvel.z * dt);

        // 地面碰撞
        if tf.translation.y < d.half.y && d.velocity.y < 0.0 {
            tf.translation.y = d.half.y;
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

    // AABB 互碰：N² 重叠 → 按质量反比位置修正 + 动量交换（简化弹性）
    // 收集所有 (entity, translation copy) 和 velocity，一次性做完再写回
    let mut trans: Vec<(Entity, Vec3)> = q.iter().map(|(e, tf, _)| (e, tf.translation)).collect();
    let mut velos: Vec<(Entity, Vec3)> = q.iter().map(|(e, _, d)| (e, d.velocity)).collect();
    for i in 0..trans.len() {
        for j in (i + 1)..trans.len() {
            let (ei, ti) = trans[i];
            let (ej, tj) = trans[j];
            let Ok(arr) = q.get_many([ei, ej]) else { continue };
            let (_, _, di) = &arr[0];
            let (_, _, dj) = &arr[1];
            let delta = ti - tj;
            let sep = di.half + dj.half;
            if delta.x.abs() < sep.x && delta.y.abs() < sep.y && delta.z.abs() < sep.z {
                let px = sep.x - delta.x.abs();
                let py = sep.y - delta.y.abs();
                let pz = sep.z - delta.z.abs();
                let (pen, s, idx) = if px < py && px < pz {
                    (px, delta.x.signum(), 0)
                } else if py < pz {
                    (py, delta.y.signum(), 1)
                } else {
                    (pz, delta.z.signum(), 2)
                };
                let tm = di.mass + dj.mass;
                if tm <= 0.0 { continue; }
                let wa = dj.mass / tm;
                let wb = di.mass / tm;
                let mut axis = Vec3::ZERO;
                axis[idx] = s;
                let push = pen * 0.5;
                trans[i].1 = ti + axis * push * wa;
                trans[j].1 = tj - axis * push * wb;
                // 速度交换：沿碰撞轴相对速度各翻一半（简化 1D 弹性）
                let v_rel = (velos[i].1[idx] - velos[j].1[idx]) * s;
                if v_rel > 0.0 {
                    let impulse = v_rel * 0.6;
                    velos[i].1[idx] -= impulse * wa;
                    velos[j].1[idx] += impulse * wb;
                }
            }
        }
    }
    // 把 trans / velos 写回
    for (e, t_new) in trans {
        if let Ok((_, mut tf, _)) = q.get_mut(e) {
            tf.translation = t_new;
        }
    }
    for (e, v_new) in velos {
        if let Ok((_, _, mut d)) = q.get_mut(e) {
            d.velocity = v_new;
        }
    }
}

// ── 演示模式：自动扔 4 块石头 → 截图 → 退出 ────────────────────────

/// 每块石头：(发射眼点偏移, 瞄准砖(c,y,x), 速度倍, seed)
const THROWS: [(Vec3, (usize, usize, usize), f32, i32); 4] = [
    // 第 1 块：砸正中心 (c=1,y=5,x=5) → 中间那层砖
    (Vec3::new(-0.9, 0.3, 0.0), (1, 5, 5), 1.0, 101),
    // 第 2 块：砸 (1,4,7) → 中下，连锁把邻居也带碎
    (Vec3::new( 0.8, 0.1, 0.0), (1, 4, 7), 1.1, 102),
    // 第 3 块：砸 (1,7,4) → 上层
    (Vec3::new( 0.0, 0.9, 0.0), (1, 7, 4), 1.0, 103),
    // 第 4 块：超高速，打穿 2 砖深小隧道
    (Vec3::new(-0.2,-0.2, 0.0), (1, 4, 5), 1.5, 104),
];

fn demo_throw(
    time: Res<Time>,
    mut demo: ResMut<DemoDriver>,
    mut throws: MessageWriter<ThrowRock>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) {
    let t = time.elapsed_secs();
    let i = demo.actions_done;
    if i >= THROWS.len() { return; }
    let (t_eye, t_target_brick, t_speed_mult, seed) = THROWS[i];
    if t >= 0.4 + 0.55 * i as f32 {
        let Ok((cam, cam_tf)) = cameras.single() else { return };

        // 目标点 = 指定砖块中心
        let (bc, by, bx) = t_target_brick;
        let target = wall::block_center(bc, by, bx);

        // 眼点：相机位置 + 偏移
        let right = cam_tf.right() * t_eye.x;
        let up    = cam_tf.up()    * t_eye.y;
        let fwd   = cam_tf.forward() * t_eye.z;
        let origin = cam_tf.translation() + right + up + fwd;

        // 方向 = target - origin，带一点向上抛（石头自带重力下坠，所以目标再抬 0.3m
        // 让抛物线落回目标高度附近）
        let aim = target + Vec3::Y * 0.30 - origin;
        let dir = aim.normalize_or_zero();
        if dir == Vec3::ZERO { return; }

        throws.write(ThrowRock {
            origin,
            direction: dir,
            speed: 25.0 * t_speed_mult,
            seed,
        });
        demo.actions_done += 1;
    }
}

fn demo_shot(time: Res<Time>, mut demo: ResMut<DemoDriver>, mut commands: Commands) {
    if time.elapsed_secs() >= 3.6 && !demo.shot_taken {
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
    if time.elapsed_secs() > 4.4 {
        println!("\n=== [{CASE}] 演示结束（冲击破坏）：最终墙态 ===");
        println!("  · 投掷机制：左键扔 IrregularRock（带质量 {}kg / 抗冲击 {}），25 m/s",
                 IrregularRock::MASS, IrregularRock::IMPACT_RESISTANCE);
        println!("  · 抗冲击阈值：Brick={}  CementBlock={}  DebrisPiece={}",
                 Brick::IMPACT_RESISTANCE, CementBlock::IMPACT_RESISTANCE, DebrisPiece::IMPACT_RESISTANCE);
        println!("  · 冲击判定：动能 0.5·m·v² ≥ 阈值 → 碎；富余超过 80% 阈值 → 邻居也碎（连锁）");
        println!("  · 右键：手敲单块砖（老逻辑，零投掷物）");
        wall::print_wall_section(&*wall, 1);
        request_exit(&mut exit);
    }
}

// 让 CurvedCylinderTrunk 的 import 不 warning（留着以后做撞树碎木头 case 直接复用）
#[allow(dead_code)]
fn _unused_trunk_ref() { let _ = CurvedCylinderTrunk::MASS; }
