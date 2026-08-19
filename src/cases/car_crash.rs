//! Case 05: car_crash —— 汽车撞击（整体刚体 + 零件级破坏 + Voxel 级玻璃粉碎）。

use bevy::app::AppExit;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::prelude::*;

use destr::common::{
    add_default_plugins, merge_flat, plain_material, spawn_camera_at, spawn_ground, spawn_sun, tint,
};
use destr::demo::{request_exit, request_screenshot, DemoDriver};
use destr::elements::{
    half_after_scale, Brick, DebrisPiece, Element,
};
use destr::tex::hash2;

const CASE: &str = "car_crash";

#[derive(Message)]
struct CarResetMsg;

fn main() {
    let demo = std::env::args().any(|a| a == "--demo");
    let mut app = App::new();
    add_default_plugins(&mut app, format!("destr · {CASE} — 整体汽车撞墙"));
    app.insert_resource(DemoDriver::default())
        .add_message::<CarResetMsg>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                input_system,
                car_rigidbody_step,
                car_wall_impact,
                detached_debris_step,
                reset_car,
            ),
        );
    if demo {
        app.add_systems(Update, (demo_drive, demo_shot, demo_exit));
    }
    app.run();
}

// ── 零件索引 ──────────────────────────────────────────────────────

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum Part {
    Body = 0,
    Hood = 1,
    BumperFront = 2,
    Trunk = 3,
    MirrorL = 4,
    MirrorR = 5,
}

fn part_profile(p: Part) -> (f32, f32, u32) {
    use Part::*;
    match p {
        Body        => (9999.0, 0.0, 0xb5312a),
        Hood        => (2600.0, 0.70, 0xb5312a),
        BumperFront => (900.0,  1.10, 0x1f1f22),
        Trunk       => (3100.0, 0.55, 0xb5312a),
        MirrorL     => (420.0,  1.40, 0x8a8a90),
        MirrorR     => (420.0,  1.40, 0x8a8a90),
    }
}

// ── 组件 ──────────────────────────────────────────────────────────

#[derive(Component)]
struct CarRigidBody {
    velocity: Vec3,
    angvel: Vec3,
    mass: f32,
    half: Vec3,
    health: f32,
}

#[derive(Component)]
struct CarRoot;

#[derive(Component)]
struct AttachedPart {
    which: Part,
    health: f32,
    max_health: f32,
    local_offset: Vec3,
}

#[derive(Component)]
struct DetachedDebris {
    velocity: Vec3,
    angvel: Vec3,
    half: Vec3,
    mass: f32,
}

#[derive(Component, Clone)]
struct WindshieldGrid {
    cols: usize,
    rows: usize,
    data: Vec<bool>,
}
impl WindshieldGrid {
    fn new(cols: usize, rows: usize) -> Self {
        Self { cols, rows, data: vec![true; cols * rows] }
    }
    fn alive(&self, c: usize, r: usize) -> bool {
        self.data[r * self.cols + c]
    }
    fn destroy(&mut self, c: usize, r: usize) {
        self.data[r * self.cols + c] = false;
    }
    fn world_center(&self, c: usize, r: usize, tf: &GlobalTransform, tile: f32) -> Vec3 {
        let w = self.cols as f32 * tile;
        let lx = -w / 2.0 + tile / 2.0 + c as f32 * tile;
        let ly = tile / 2.0 + r as f32 * tile;
        tf.transform_point(Vec3::new(lx, ly, 0.0))
    }
    fn count_alive(&self) -> usize {
        self.data.iter().filter(|&&v| v).count()
    }
}

#[derive(Resource, Clone)]
struct BrickWallTarget {
    cols: usize,
    rows: usize,
    layers: usize,
    data: Vec<bool>,
}
impl BrickWallTarget {
    fn new(cols: usize, rows: usize, layers: usize) -> Self {
        Self { cols, rows, layers, data: vec![true; cols * rows * layers] }
    }
    fn idx(&self, x: usize, y: usize, c: usize) -> usize {
        c * self.rows * self.cols + y * self.cols + x
    }
    fn alive(&self, x: usize, y: usize, c: usize) -> bool {
        self.data[self.idx(x, y, c)]
    }
    fn destroy(&mut self, x: usize, y: usize, c: usize) {
        let i = self.idx(x, y, c);
        self.data[i] = false;
    }
    fn block_center(&self, x: usize, y: usize, c: usize) -> Vec3 {
        let bw = Brick::WIDTH;
        let bh = Brick::HEIGHT;
        let bd = Brick::DEPTH;
        let wall_w = self.cols as f32 * bw;
        let wall_t = self.layers as f32 * bd;
        let bond = if y % 2 == 1 { bw * 0.5 } else { 0.0 };
        Vec3::new(
            -wall_w / 2.0 + bw / 2.0 + x as f32 * bw + bond,
            bh / 2.0 + y as f32 * bh,
            8.0 - wall_t / 2.0 + bd / 2.0 + c as f32 * bd,
        )
    }
    fn block_from_point(&self, p: Vec3) -> Option<(usize, usize, usize)> {
        let bw = Brick::WIDTH;
        let bh = Brick::HEIGHT;
        let bd = Brick::DEPTH;
        let wall_w = self.cols as f32 * bw;
        let wall_t = self.layers as f32 * bd;
        let wall_z_center = 8.0;
        if p.x < -wall_w / 2.0 - bw * 0.6 || p.x > wall_w / 2.0 + bw * 0.6 { return None; }
        if p.z < wall_z_center - wall_t / 2.0 - 0.3 || p.z > wall_z_center + wall_t / 2.0 + 0.6 { return None; }
        let y = (p.y / bh).floor() as i32;
        if y < 0 || y as usize >= self.rows { return None; }
        let y = y as usize;
        let bond = if y % 2 == 1 { bw * 0.5 } else { 0.0 };
        let x = ((p.x + wall_w / 2.0 - bond) / bw).floor() as i32;
        let x = x.clamp(0, self.cols as i32 - 1) as usize;
        let c = ((p.z - (wall_z_center - wall_t / 2.0)) / bd).floor() as i32;
        let c = c.clamp(0, self.layers as i32 - 1) as usize;
        Some((x, y, c))
    }
}

// ── Startup ───────────────────────────────────────────────────────

fn hex4(hex: u32) -> [f32; 4] {
    let r = ((hex >> 16) & 0xff) as f32 / 255.0;
    let g = ((hex >> 8) & 0xff) as f32 / 255.0;
    let b = (hex & 0xff) as f32 / 255.0;
    fn f(u: f32) -> f32 {
        if u <= 0.040_45 { u / 12.92 } else { ((u + 0.055) / 1.055).powf(2.4) }
    }
    [f(r), f(g), f(b), 1.0]
}

const CAR_HALF_W: f32 = 0.95;
const CAR_HALF_H: f32 = 0.55;
const CAR_HALF_L: f32 = 2.10;
const CAR_CENTER_Y: f32 = 0.70;

const CAR_START: Vec3 = Vec3::new(0.0, CAR_CENTER_Y, -12.0);

#[derive(Resource)]
struct CarAssets {
    paint_mat: Handle<StandardMaterial>,
    plastic_mat: Handle<StandardMaterial>,
    glass_mat: Handle<StandardMaterial>,
    wheel_mat: Handle<StandardMaterial>,
    debris_mesh: Handle<Mesh>,
    brick_mat: Handle<StandardMaterial>,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    spawn_sun(&mut commands);
    let plain = plain_material(&mut materials);
    spawn_ground(&mut commands, &mut meshes, plain);
    spawn_camera_at(&mut commands, Vec3::new(-10.0, 6.5, -6.0), Vec3::new(0.0, 1.3, -1.0));

    let paint = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(0xb5, 0x31, 0x2a),
        perceptual_roughness: 0.35,
        metallic: 0.25,
        ..default()
    });
    let plastic = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(0x1f, 0x1f, 0x22),
        perceptual_roughness: 0.88,
        ..default()
    });
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgba(0.50, 0.68, 0.78, 0.45),
        perceptual_roughness: 0.08,
        metallic: 0.05,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let wheel = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(0x14, 0x14, 0x16),
        perceptual_roughness: 0.95,
        ..default()
    });
    let debris_mesh_h = meshes.add(DebrisPiece::base_mesh());
    let brick_mat = <Brick as Element>::default_material(&mut materials, &mut images, Color::WHITE, 0.95);

    commands.insert_resource(CarAssets {
        paint_mat: paint.clone(),
        plastic_mat: plastic.clone(),
        glass_mat: glass.clone(),
        wheel_mat: wheel.clone(),
        debris_mesh: debris_mesh_h.clone(),
        brick_mat: brick_mat.clone(),
    });

    let wall = BrickWallTarget::new(8, 6, 2);
    build_wall_visuals(&wall, &mut commands, &mut meshes, brick_mat.clone());
    commands.insert_resource(wall);

    spawn_car(&mut commands, &mut meshes, &CarAssets {
        paint_mat: paint,
        plastic_mat: plastic,
        glass_mat: glass,
        wheel_mat: wheel,
        debris_mesh: debris_mesh_h,
        brick_mat,
    });
}

fn build_wall_visuals(
    wall: &BrickWallTarget,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    brick_mat: Handle<StandardMaterial>,
) {
    let mut parts: Vec<(Mesh, [f32; 4])> = Vec::new();
    for c in 0..wall.layers {
        for y in 0..wall.rows {
            for x in 0..wall.cols {
                if !wall.alive(x, y, c) { continue; }
                let center = wall.block_center(x, y, c);
                let m = <Brick as Element>::painted_mesh(x as i32, y as i32, c as i32, 0, center, true);
                parts.push((m, [1.0, 1.0, 1.0, 1.0]));
            }
        }
    }
    let mesh = if parts.is_empty() {
        destr::elements::empty_triangle_mesh()
    } else {
        merge_flat(parts)
    };
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(brick_mat),
        Transform::default(),
        Name::new("target_wall_visual"),
    ));
}

fn rebuild_wall_visuals(
    wall: &BrickWallTarget,
    q: &mut Query<(Entity, &mut Mesh3d, Option<&Name>)>,
    meshes: &mut Assets<Mesh>,
) {
    let mut found_entity: Option<Entity> = None;
    for (e, _m, name_opt) in q.iter_mut() {
        if let Some(n) = name_opt {
            if n.as_str() == "target_wall_visual" {
                found_entity = Some(e);
                break;
            }
        }
    }
    let mut parts: Vec<(Mesh, [f32; 4])> = Vec::new();
    for c in 0..wall.layers {
        for y in 0..wall.rows {
            for x in 0..wall.cols {
                if !wall.alive(x, y, c) { continue; }
                let center = wall.block_center(x, y, c);
                let m = <Brick as Element>::painted_mesh(x as i32, y as i32, c as i32, 0, center, true);
                parts.push((m, [1.0, 1.0, 1.0, 1.0]));
            }
        }
    }
    let mesh = if parts.is_empty() {
        destr::elements::empty_triangle_mesh()
    } else {
        merge_flat(parts)
    };
    if let Some(e) = found_entity {
        if let Ok((_, mut m, _)) = q.get_mut(e) {
            **m = meshes.add(mesh);
        }
    }
}

fn spawn_car(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    assets: &CarAssets,
) {
    let body_cuboid = Cuboid::new(CAR_HALF_W * 2.0, CAR_HALF_H * 2.0, CAR_HALF_L * 2.0).mesh().build();
    let roof_cuboid = Cuboid::new(CAR_HALF_W * 1.7, CAR_HALF_H * 1.1, CAR_HALF_L * 1.15).mesh().build();
    // 用 Transform 的方式把 roof 挪位：transform_point 批量操作顶点
    let roof_mesh = roof_cuboid.translated_by(Vec3::new(0.0, CAR_HALF_H * 0.75, -CAR_HALF_L * 0.08));
    let body_mesh = merge_flat(vec![
        (body_cuboid, hex4(part_profile(Part::Body).2)),
        (roof_mesh, hex4(part_profile(Part::Body).2)),
    ]);

    let car_root = commands.spawn((
        Mesh3d(meshes.add(body_mesh)),
        MeshMaterial3d(assets.paint_mat.clone()),
        Transform::from_translation(CAR_START),
        CarRigidBody {
            velocity: Vec3::ZERO,
            angvel: Vec3::ZERO,
            mass: 1500.0,
            half: Vec3::new(CAR_HALF_W, CAR_HALF_H + CAR_HALF_H * 0.75, CAR_HALF_L),
            health: 0.0,
        },
        CarRoot,
        Name::new("car_root"),
    )).id();

    let wheel_r = 0.32;
    let wheel_h = 0.22;
    let cylinder_mesh = Cylinder::new(wheel_r, wheel_h).mesh().build();
    let wheel_mesh = cylinder_mesh.rotated_by(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
    let wheel_positions = [
        (-CAR_HALF_W + wheel_h * 0.4, -CAR_HALF_H,  CAR_HALF_L - wheel_r * 0.9),
        ( CAR_HALF_W - wheel_h * 0.4, -CAR_HALF_H,  CAR_HALF_L - wheel_r * 0.9),
        (-CAR_HALF_W + wheel_h * 0.4, -CAR_HALF_H, -CAR_HALF_L + wheel_r * 0.9),
        ( CAR_HALF_W - wheel_h * 0.4, -CAR_HALF_H, -CAR_HALF_L + wheel_r * 0.9),
    ];
    for (i, &(x, y, z)) in wheel_positions.iter().enumerate() {
        let e = commands.spawn((
            Mesh3d(meshes.add(wheel_mesh.clone())),
            MeshMaterial3d(assets.wheel_mat.clone()),
            Transform::from_translation(Vec3::new(x, y, z)),
            Name::new(format!("wheel_{}", ["FL", "FR", "RL", "RR"][i])),
        )).id();
        commands.entity(car_root).add_child(e);
    }

    let mut spawn_part = |which: Part, mesh: Mesh, local_offset: Vec3| {
        let (hp, _, color) = part_profile(which);
        let mesh = tint(mesh, hex4(color));
        let mat = match which {
            Part::BumperFront | Part::MirrorL | Part::MirrorR => assets.plastic_mat.clone(),
            _ => assets.paint_mat.clone(),
        };
        let e = commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(mat),
            Transform::from_translation(local_offset),
            AttachedPart {
                which,
                health: hp,
                max_health: hp,
                local_offset,
            },
            Name::new(format!("part_{:?}", which)),
        )).id();
        commands.entity(car_root).add_child(e);
    };

    let hood_mesh = Cuboid::new(CAR_HALF_W * 1.9, 0.05, CAR_HALF_L * 0.65).mesh().build();
    spawn_part(Part::Hood, hood_mesh, Vec3::new(0.0, CAR_HALF_H - 0.01, CAR_HALF_L * 0.45));

    let bumper_mesh = Cuboid::new(CAR_HALF_W * 2.05, 0.16, 0.12).mesh().build();
    spawn_part(Part::BumperFront, bumper_mesh, Vec3::new(0.0, -CAR_HALF_H * 0.45, CAR_HALF_L + 0.04));

    let trunk_mesh = Cuboid::new(CAR_HALF_W * 1.9, 0.05, CAR_HALF_L * 0.55).mesh().build();
    spawn_part(Part::Trunk, trunk_mesh, Vec3::new(0.0, CAR_HALF_H + CAR_HALF_H * 0.55, -CAR_HALF_L * 0.40));

    let mirror_mesh_l = Cuboid::new(0.08, 0.14, 0.20).mesh().build();
    let mirror_mesh_r = Cuboid::new(0.08, 0.14, 0.20).mesh().build();
    spawn_part(Part::MirrorL, mirror_mesh_l, Vec3::new(-CAR_HALF_W - 0.06, CAR_HALF_H * 0.15, CAR_HALF_L * 0.20));
    spawn_part(Part::MirrorR, mirror_mesh_r, Vec3::new( CAR_HALF_W + 0.06, CAR_HALF_H * 0.15, CAR_HALF_L * 0.20));

    // 挡风玻璃 5×4 Voxel
    let tile = 0.18;
    let cols = 5usize;
    let rows = 4usize;
    let grid = WindshieldGrid::new(cols, rows);
    let tile_mesh = Cuboid::new(tile, tile, 0.03).mesh().build();
    let mut parts: Vec<(Mesh, [f32; 4])> = Vec::new();
    let w = cols as f32 * tile;
    for r in 0..rows {
        for c in 0..cols {
            let lx = -w / 2.0 + tile / 2.0 + c as f32 * tile;
            let ly = tile / 2.0 + r as f32 * tile;
            let m = tile_mesh.clone().translated_by(Vec3::new(lx, ly, 0.0));
            let tv = 0.82 + hash2(c as i32, r as i32) * 0.18;
            parts.push((m, [0.8 * tv, 0.9 * tv, 1.0 * tv, 1.0]));
        }
    }
    let merged = merge_flat(parts);
    let glass_local_pos = Vec3::new(0.0, CAR_HALF_H * 0.85, CAR_HALF_L * 0.18);
    let glass_entity = commands.spawn((
        Mesh3d(meshes.add(merged)),
        MeshMaterial3d(assets.glass_mat.clone()),
        Transform::from_translation(glass_local_pos),
        grid,
        Name::new("windshield_grid"),
    )).id();
    commands.entity(car_root).add_child(glass_entity);
}

// ── 输入系统 ──────────────────────────────────────────────────────

fn input_system(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut car: Query<&mut CarRigidBody, With<CarRoot>>,
    mut reset: MessageWriter<CarResetMsg>,
) {
    if keys.just_pressed(KeyCode::Space) {
        reset.write(CarResetMsg);
    }
    let accel = if mouse.pressed(MouseButton::Left) || keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        7.5
    } else {
        0.0
    };
    let brake = keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown);
    for mut rb in &mut car {
        rb.velocity.z += accel;   // 每帧一次性加一下
        if brake {
            rb.velocity.z *= 0.92;
        }
    }
}

// ── 复位车 ────────────────────────────────────────────────────────

fn reset_car(
    mut events: MessageReader<CarResetMsg>,
    mut car: Query<(&mut CarRigidBody, &mut Transform), With<CarRoot>>,
    mut commands: Commands,
    detach_q: Query<Entity, With<DetachedDebris>>,
) {
    let mut need = false;
    for _ in events.read() { need = true; }
    if !need { return; }
    for (mut rb, mut tf) in &mut car {
        rb.velocity = Vec3::ZERO;
        rb.angvel = Vec3::ZERO;
        rb.health = 0.0;
        tf.translation = CAR_START;
        tf.rotation = Quat::IDENTITY;
    }
    for e in &detach_q {
        commands.entity(e).despawn();
    }
}

// ── 主刚体 ────────────────────────────────────────────────────────

fn car_rigidbody_step(
    time: Res<Time>,
    mut car: Query<(&mut CarRigidBody, &mut Transform), With<CarRoot>>,
    mut reset: MessageWriter<CarResetMsg>,
) {
    let dt = time.delta_secs();
    for (mut rb, mut tf) in &mut car {
        rb.velocity.y -= 10.0 * dt;
        tf.translation += rb.velocity * dt;
        let v = rb.angvel * dt;
        tf.rotation = Quat::from_scaled_axis(v) * tf.rotation;

        if tf.translation.y < CAR_CENTER_Y {
            tf.translation.y = CAR_CENTER_Y;
            if rb.velocity.y < -0.5 {
                rb.velocity.y = -rb.velocity.y * 0.25;
                rb.velocity.x *= 0.9;
                rb.velocity.z *= 0.9;
                rb.angvel *= 0.7;
            } else {
                rb.velocity.y = 0.0;
            }
        }
        if tf.translation.length() > 45.0 || tf.translation.y > 8.0 {
            reset.write(CarResetMsg);
        }
    }
}

// ── 撞击检测 + 破坏 ───────────────────────────────────────────────

fn car_wall_impact(
    mut commands: Commands,
    mut car: Query<(Entity, &mut CarRigidBody, &Transform, &Children), With<CarRoot>>,
    mut parts: Query<(&mut AttachedPart, &GlobalTransform, Entity)>,
    mut wall: ResMut<BrickWallTarget>,
    mut wall_vis_q: Query<(Entity, &mut Mesh3d, Option<&Name>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    assets: Res<CarAssets>,
    mut windshields: Query<(Entity, &mut WindshieldGrid, &GlobalTransform)>,
) {
    let mut snaps: Vec<(Entity, Vec3, Vec3, Vec3, f32)> = Vec::new();
    for (e, rb, tf, _) in &car {
        snaps.push((e, rb.velocity, rb.half, tf.translation, rb.mass));
    }

    for (car_e, vel, half, pos, mass) in snaps {
        let front_z = pos.z + half.z;
        let wall_front_z_min = 8.0 - Brick::DEPTH;
        let wall_front_z_max = 8.0 + Brick::DEPTH;
        if front_z < wall_front_z_min - 0.3 { continue; }
        if pos.z - half.z > wall_front_z_max + 2.0 { continue; }

        let mut hits: Vec<(usize, usize, usize, Vec3)> = Vec::new();
        for i in 0..5 {
            for j in 0..3 {
                for k in 0..3 {
                    let t_x = (i as f32 / 4.0) * 2.0 - 1.0;
                    let t_y = (j as f32 / 2.0) * 2.0 - 1.0;
                    let t_z = 0.75 + (k as f32 / 2.0) * 0.4;
                    let probe = Vec3::new(
                        pos.x + half.x * t_x,
                        pos.y + half.y * t_y,
                        pos.z + half.z * t_z,
                    );
                    if let Some((bx, by, bc)) = wall.block_from_point(probe) {
                        if wall.alive(bx, by, bc) {
                            hits.push((bx, by, bc, probe));
                        }
                    }
                }
            }
        }
        if hits.is_empty() { return; }

        let ke = 0.5 * mass * vel.length_squared();
        if ke < 50.0 { return; }

        if let Ok((_car_e, mut rb, _tf, _ch)) = car.get_mut(car_e) {
            if rb.velocity.z > 0.0 {
                rb.velocity.z = -rb.velocity.z * 0.35;
                rb.velocity.x *= 0.4;
                rb.velocity.y = rb.velocity.y.max(1.2);
                rb.angvel = Vec3::new(
                    (hash2(7, 3) - 0.5) * 1.2,
                    (hash2(7, 5) - 0.5) * 0.6,
                    (hash2(7, 9) - 0.5) * 1.4,
                );
                rb.health += ke.min(5000.0);
            }
        }

        for (bx, by, bc, _probe) in &hits {
            if !wall.alive(*bx, *by, *bc) { continue; }
            if ke >= Brick::IMPACT_RESISTANCE {
                wall.destroy(*bx, *by, *bc);
                let bc2 = wall.block_center(*bx, *by, *bc);
                let splash = -vel.normalize_or(-Vec3::Z);
                spawn_car_debris(bc2, &assets, splash, ke, *bx as i32, *by as i32, *bc as i32, &mut commands);
            }
        }
        rebuild_wall_visuals(&*wall, &mut wall_vis_q, &mut meshes);

        let avg: Vec3 = hits.iter().map(|h| h.3).sum::<Vec3>() / hits.len() as f32;
        let Ok((_e, _rb, _tf, children)) = car.get(car_e) else { return };

        // 零件破坏
        let mut detach_list: Vec<(Entity, Part, Vec3, Vec3, f32)> = Vec::new();
        for child in children.iter() {
            if let Ok((at, gtf, e)) = parts.get_mut(child) {
                let part_world = gtf.translation();
                let d2 = (part_world - avg).length_squared();
                let (threshold, splash_mult, _color) = part_profile(at.which);
                if d2 < 4.5 && ke * (1.0 - (d2 / 6.0).min(0.9)) >= threshold * 0.45 {
                    // 因为 at 是 &mut AttachedPart，这里不能 reborrow 再 push——直接操作实体组件
                    let p = at.which;
                    let part_half = match p {
                        Part::BumperFront => Vec3::new(CAR_HALF_W * 1.03, 0.08, 0.06),
                        Part::Hood => Vec3::new(CAR_HALF_W * 0.96, 0.03, CAR_HALF_L * 0.33),
                        Part::Trunk => Vec3::new(CAR_HALF_W * 0.96, 0.03, CAR_HALF_L * 0.28),
                        Part::MirrorL | Part::MirrorR => Vec3::new(0.04, 0.07, 0.10),
                        Part::Body => Vec3::splat(0.2),
                    };
                    let part_mass: f32 = match p {
                        Part::BumperFront => 5.0,
                        Part::MirrorL | Part::MirrorR => 0.3,
                        Part::Hood | Part::Trunk => 12.0,
                        Part::Body => 100.0,
                    };
                    let dir = (part_world - avg).normalize_or(vel.normalize_or(Vec3::Z));
                    let sp = (ke / part_mass.max(1.0)).sqrt() * splash_mult;
                    drop(at);
                    detach_list.push((e, p, dir * sp + Vec3::Y * (ke / 1800.0).min(5.0), part_half, part_mass));
                }
            }
        }
        for (e, p, velo, part_half, part_mass) in detach_list {
            // 零件脱落
            commands.entity(e).remove_parent_in_place();
            commands.entity(e)
                .remove::<AttachedPart>()
                .insert(DetachedDebris {
                    velocity: velo,
                    angvel: Vec3::new(
                        (hash2(p as i32, 1) - 0.5) * 8.0,
                        (hash2(p as i32, 2) - 0.5) * 8.0,
                        (hash2(p as i32, 3) - 0.5) * 8.0,
                    ),
                    half: part_half,
                    mass: part_mass,
                });
        }

        // 玻璃破坏
        for child in children.iter() {
            if let Ok((glass_e, mut grid, gtf)) = windshields.get_mut(child) {
                let tile = 0.18;
                let mut destroyed_any = false;
                for r in 0..grid.rows {
                    for c in 0..grid.cols {
                        if !grid.alive(c, r) { continue; }
                        let cp = grid.world_center(c, r, &gtf, tile);
                        let d2 = (cp - avg).length_squared();
                        if d2 < 1.8 && ke > 120.0 {
                            grid.destroy(c, r);
                            destroyed_any = true;
                            let splash = -vel.normalize_or(-Vec3::Z);
                            spawn_car_debris(cp, &assets, splash, ke * 0.3, c as i32, r as i32, 0, &mut commands);
                        }
                    }
                }
                if destroyed_any {
                    let tile_mesh_src = Cuboid::new(tile, tile, 0.03).mesh().build();
                    let mut parts: Vec<(Mesh, [f32; 4])> = Vec::new();
                    let w = grid.cols as f32 * tile;
                    for r in 0..grid.rows {
                        for c in 0..grid.cols {
                            if !grid.alive(c, r) { continue; }
                            let lx = -w / 2.0 + tile / 2.0 + c as f32 * tile;
                            let ly = tile / 2.0 + r as f32 * tile;
                            let m = tile_mesh_src.clone().translated_by(Vec3::new(lx, ly, 0.0));
                            let tv = 0.82 + hash2(c as i32, r as i32) * 0.18;
                            parts.push((m, [0.8 * tv, 0.9 * tv, 1.0 * tv, 1.0]));
                        }
                    }
                    let merged = if parts.is_empty() {
                        destr::elements::empty_triangle_mesh()
                    } else {
                        merge_flat(parts)
                    };
                    commands.entity(glass_e).insert(Mesh3d(meshes.add(merged)));
                }
            }
        }
    }
}

fn spawn_car_debris(
    pos: Vec3,
    assets: &CarAssets,
    splash: Vec3,
    ke: f32,
    sx: i32, sy: i32, sz: i32,
    commands: &mut Commands,
) {
    let n = if ke > 3000.0 { 4 } else { 3 };
    for i in 0..n {
        let h1 = hash2(sx + i, sy * 3 + sz);
        let h2 = hash2(sy * 5 + i, sz * 7 + sx);
        let h3 = hash2(sz * 11 + i, sx * 2 + sy);
        let scale = 0.55 + h1 * 0.7;
        let mass = DebrisPiece::MASS * scale;
        let half = half_after_scale(DebrisPiece::SIZE, Vec3::splat(scale));
        let speed = 2.0 + (ke / 250.0).min(9.0) + h2 * 3.0;
        let dir = (splash + Vec3::new((h1 - 0.5) * 1.2, 0.4 + h2 * 0.8, (h3 - 0.5) * 1.2)).normalize_or(Vec3::Y);
        commands.spawn((
            Mesh3d(assets.debris_mesh.clone()),
            MeshMaterial3d(assets.brick_mat.clone()),
            Transform::from_translation(pos + Vec3::new((h1 - 0.5) * 0.4, (h2 - 0.5) * 0.3, (h3 - 0.5) * 0.4))
                .with_scale(Vec3::splat(scale)),
            DetachedDebris {
                velocity: dir * speed,
                angvel: Vec3::new((h1 - 0.5) * 6.0, (h2 - 0.5) * 6.0, (h3 - 0.5) * 6.0),
                half,
                mass,
            },
            Name::new(format!("car_debris_{:02}", i)),
        ));
    }
}

// ── Debris step ───────────────────────────────────────────────────

fn detached_debris_step(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut DetachedDebris)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let g = 14.0;
    let mut trans: Vec<(Entity, Vec3)> = q.iter().map(|(e, tf, _)| (e, tf.translation)).collect();
    let mut velos: Vec<(Entity, Vec3)> = q.iter().map(|(e, _, d)| (e, d.velocity)).collect();

    for (e, mut tf, mut d) in &mut q {
        d.velocity.y -= g * dt;
        tf.translation += d.velocity * dt;
        let av = d.angvel * dt;
        tf.rotation = Quat::from_scaled_axis(av) * tf.rotation;

        if tf.translation.y < d.half.y && d.velocity.y < 0.0 {
            tf.translation.y = d.half.y;
            if d.velocity.y < -2.0 {
                d.velocity.y = -d.velocity.y * 0.35;
                d.velocity.x *= 0.6;
                d.velocity.z *= 0.6;
                d.angvel *= 0.55;
            } else {
                commands.entity(e).remove::<DetachedDebris>();
            }
        }
        if tf.translation.length() > 80.0 {
            commands.entity(e).despawn();
        }
    }

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
                let v_rel = (velos[i].1[idx] - velos[j].1[idx]) * s;
                if v_rel > 0.0 {
                    let imp = v_rel * 0.55;
                    velos[i].1[idx] -= imp * wa;
                    velos[j].1[idx] += imp * wb;
                }
            }
        }
    }
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

// ── Demo 模式 ─────────────────────────────────────────────────────

fn demo_drive(
    time: Res<Time>,
    mut car: Query<&mut CarRigidBody, With<CarRoot>>,
) {
    let t = time.elapsed_secs();
    if t > 0.6 {
        for mut rb in &mut car {
            rb.velocity.z += 7.0 * time.delta_secs();
            if rb.velocity.z > 20.0 { rb.velocity.z = 20.0; }
        }
    }
}

fn demo_shot(time: Res<Time>, mut demo: ResMut<DemoDriver>, mut commands: Commands) {
    if time.elapsed_secs() >= 2.8 && !demo.shot_taken {
        demo.shot_taken = true;
        destr::demo::shot_marker(CASE);
        request_screenshot(&mut commands, CASE);
    }
}

fn demo_exit(
    time: Res<Time>,
    wall: Res<BrickWallTarget>,
    mut exit: MessageWriter<AppExit>,
    grid_q: Query<&WindshieldGrid>,
    detached_cnt: Query<&DetachedDebris>,
) {
    if time.elapsed_secs() > 3.6 {
        let total = wall.cols * wall.rows * wall.layers;
        let alive = wall.data.iter().filter(|&&v| v).count();
        let destroyed_bricks = total - alive;
        let mut detached = 0i32;
        for _ in &detached_cnt { detached += 1; }
        let gs = grid_q.iter().next();
        let (g_total, g_alive) = match gs {
            Some(g) => (g.cols * g.rows, g.count_alive()),
            None => (0, 0),
        };
        println!("\n=== [{CASE}] 汽车撞击演示结束 ===");
        println!("  · 架构：车身主 CarRigidBody（1500kg 整体刚体 + 子零件）—— 证明为什么车不适合纯小方块拼");
        println!("  · 撞击判定：车头 AABB 5×3×3 探针扫墙，动能 0.5·m·v² ≥ 阈值（砖 520 J）就碎");
        println!("  · 墙：{}/{} 块砖", alive, total);
        println!("    - 本次撞毁：{} 块（砖墙级，Voxel 消失式破坏）", destroyed_bricks);
        println!("  · 零件级破坏（零件≠小方块）：目前 detached Debris {} 块", detached);
        println!("  · 挡风玻璃（Voxel 小面板）：{}/{} 块", g_alive, g_total);
        println!("  · 结论：车=整体刚体，墙和玻璃=小方块——混合架构各司其职 ✅");
        request_exit(&mut exit);
    }
}
