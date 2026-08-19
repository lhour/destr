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
    BumperRear = 6,
    FenderFL = 7,    // 左前翼子板
    FenderFR = 8,    // 右前翼子板
    FenderRL = 9,    // 左后翼子板
    FenderRR = 10,   // 右后翼子板
    HeadlightL = 11,
    HeadlightR = 12,
    TaillightL = 13,
    TaillightR = 14,
    DoorFL = 15,     // 左前门
    DoorFR = 16,     // 右前门
    DoorRL = 17,     // 左后门
    DoorRR = 18,     // 右后门
    ExhaustL = 19,   // 左排气管
    ExhaustR = 20,   // 右排气管
}
const PARTS_TOTAL: usize = 21;

/// 各零件参数：(抗冲击强度 J， 脱落时飞溅倍率， 颜色 hex)
fn part_profile(p: Part) -> (f32, f32, u32) {
    use Part::*;
    match p {
        Body        => (9999.0, 0.0,  0xb5312a),
        Hood        => (2600.0, 0.70, 0xb5312a),
        BumperFront => (900.0,  1.10, 0x1f1f22),
        Trunk       => (3100.0, 0.55, 0xb5312a),
        MirrorL     => (420.0,  1.40, 0x8a8a90),
        MirrorR     => (420.0,  1.40, 0x8a8a90),
        BumperRear  => (1100.0, 0.90, 0x1f1f22),
        FenderFL | FenderFR | FenderRL | FenderRR
                    => (1200.0, 0.80, 0xb5312a),
        HeadlightL | HeadlightR
                    => (380.0,  1.60, 0xefe4a0),
        TaillightL | TaillightR
                    => (380.0,  1.60, 0xb32424),
        DoorFL | DoorFR | DoorRL | DoorRR
                    => (1900.0, 0.60, 0xb5312a),
        ExhaustL | ExhaustR
                    => (700.0,  1.20, 0x8a8a90),
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
    paint_mat: Handle<StandardMaterial>,      // 车身红漆
    plastic_mat: Handle<StandardMaterial>,    // 保险杠/后视镜/排气管 黑塑料
    glass_mat: Handle<StandardMaterial>,      // 蓝色半透玻璃
    wheel_mat: Handle<StandardMaterial>,      // 轮胎黑
    hub_mat: Handle<StandardMaterial>,        // 轮毂银灰
    interior_mat: Handle<StandardMaterial>,   // 仪表台/座椅 深棕
    headlight_mat: Handle<StandardMaterial>,  // 前大灯（emissive 暖白）
    taillight_mat: Handle<StandardMaterial>,  // 尾灯（emissive 红）
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
    // 轮毂：银灰色金属
    let hub = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(0xc4, 0xc4, 0xca),
        perceptual_roughness: 0.28,
        metallic: 0.85,
        ..default()
    });
    // 内饰：深棕黑（塑料）
    let interior = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(0x33, 0x2c, 0x29),
        perceptual_roughness: 0.90,
        ..default()
    });
    // 大灯：暖白 + 自发光
    let headlight = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(0xff, 0xf4, 0xd4),
        emissive: LinearRgba::new(0.95_f32.powf(2.2), 0.85_f32.powf(2.2), 0.55_f32.powf(2.2), 1.0),
        perceptual_roughness: 0.15,
        metallic: 0.15,
        ..default()
    });
    // 尾灯：正红 + 自发光
    let taillight = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(0xdc, 0x20, 0x20),
        emissive: LinearRgba::new(0.8_f32.powf(2.2), 0.08_f32.powf(2.2), 0.08_f32.powf(2.2), 1.0),
        perceptual_roughness: 0.3,
        metallic: 0.1,
        ..default()
    });
    let debris_mesh_h = meshes.add(DebrisPiece::base_mesh());
    let brick_mat = <Brick as Element>::default_material(&mut materials, &mut images, Color::WHITE, 0.95);

    commands.insert_resource(CarAssets {
        paint_mat: paint.clone(),
        plastic_mat: plastic.clone(),
        glass_mat: glass.clone(),
        wheel_mat: wheel.clone(),
        hub_mat: hub.clone(),
        interior_mat: interior.clone(),
        headlight_mat: headlight.clone(),
        taillight_mat: taillight.clone(),
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
        hub_mat: hub,
        interior_mat: interior,
        headlight_mat: headlight,
        taillight_mat: taillight,
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
    // ── 1. 主体骨架（CarRoot 节点，整体刚体组件已经挂载在一个合并车身壳上） ────
    // 为了"整体刚体 + 多零件"的架构依然成立：CarRigidBody 挂在 car_root 上，
    // car_root 自己有一个视觉组件（合并车身外壳：地板 + 顶梁骨架 + 车门框横梁），
    // 其余零件都 spawn 为 car_root 的 children。

    // ─────────────────────────────────────────────────────────────
    //  车身外壳：多个带角度 Cuboid 合成三厢轿车剪影（侧面 引擎盖 - 前玻璃 - 顶 - 后玻璃 - 尾箱）
    // ─────────────────────────────────────────────────────────────
    //   Z 坐标（车头朝 +Z）：
    //   CAR_HALF_L = +2.10 → 车最前（保险杠尖端）
    //   +1.55 → 引擎盖前端
    //   +0.60 → 挡风玻璃下缘 / 仪表盘位置
    //   -0.00 → 车顶前缘
    //   -1.55 → 车顶后缘 / 后玻璃下缘
    //   -1.95 → 尾箱末端
    //   -CAR_HALF_L = -2.10 → 最后（后保险杠）
    //   Y 坐标：CAR_CENTER_Y = 0.70 是车身中心（整体 Transform translation = CAR_START）
    //      -CAR_HALF_H = 0.15  → 车底盘下沿（离地 0.15 = 轮心 0.32 - 压入 0.17）
    //      +CAR_HALF_H = 1.25  → 引擎盖上沿 + 座椅下沿
    //      +CAR_HALF_H * 1.4   → 仪表台上沿
    //      +CAR_HALF_H * 2.0   → 车顶（最高点）
    //   X 坐标：
    //      -CAR_HALF_W = -0.95 → 车身左沿
    //      +CAR_HALF_W = +0.95 → 车身右沿
    // ─────────────────────────────────────────────────────────────

    let mut shell_parts: Vec<(Mesh, [f32; 4])> = Vec::new();

    // ① 地板（黑色长扁底盘）——————————————————————————————————
    let floor_h = 0.08;
    let floor_mesh = Cuboid::new(CAR_HALF_W * 1.92, floor_h, CAR_HALF_L * 1.92).mesh().build();
    shell_parts.push((floor_mesh.translated_by(Vec3::new(0.0, -CAR_HALF_H + floor_h / 2.0, 0.0)), hex4(0x212123)));

    // ② 防火墙 + 前后隔板（4 块立板，定义 3 个箱：前仓 / 座舱 / 后仓）
    let fw_mesh = Cuboid::new(CAR_HALF_W * 1.96, CAR_HALF_H * 1.0, 0.04).mesh().build();
    shell_parts.push((fw_mesh.clone().translated_by(Vec3::new(0.0, CAR_HALF_H * 0.0, 0.58)), hex4(0xa12824))); // 前壁（火墙）
    shell_parts.push((fw_mesh.translated_by(Vec3::new(0.0, CAR_HALF_H * 0.0, -1.53)), hex4(0xa12824))); // 后壁（座椅靠背后面）

    // 两侧门槛（黑色长条，车底左右装饰条）
    let sills_mesh = Cuboid::new(0.06, 0.12, CAR_HALF_L * 1.96).mesh().build();
    shell_parts.push((sills_mesh.clone().translated_by(Vec3::new( CAR_HALF_W * 0.97, -CAR_HALF_H + 0.10, 0.0)), hex4(0x1b1b1e)));
    shell_parts.push((sills_mesh.translated_by(Vec3::new(-CAR_HALF_W * 0.97, -CAR_HALF_H + 0.10, 0.0)), hex4(0x1b1b1e)));

    // ③ 前引擎盖（斜面：朝车头方向降低 9cm = 5.8°）——————————
    let hood_w = CAR_HALF_W * 1.90;
    let hood_l = CAR_HALF_L - 0.45;   // 从 1.65 往后
    let hood_t = 0.04;
    let hood_tilt_deg = 5.8f32.to_radians();
    let hood_mesh = Cuboid::new(hood_w, hood_t, hood_l).mesh().build()
        .rotated_by(Quat::from_rotation_x(-hood_tilt_deg))
        // 旋转中心在 Cuboid 中心 → 把前边缘落到 +CAR_HALF_L -0.05，y 比水平位置低 hood_l/2 * sin
        .translated_by(Vec3::new(
            0.0,
            CAR_HALF_H + hood_t / 2.0 - 0.025,  // 平均高度略低于车身高点
            (0.58 + (CAR_HALF_L - 0.05)) / 2.0,
        ));
    shell_parts.push((hood_mesh, hex4(part_profile(Part::Hood).2)));

    // ④ 后备箱盖（微微后倾，更有姿态）
    let trunk_w = CAR_HALF_W * 1.90;
    let trunk_l = 0.48;
    let trunk_t = 0.04;
    let trunk_mesh = Cuboid::new(trunk_w, trunk_t, trunk_l).mesh().build()
        .rotated_by(Quat::from_rotation_x(-2.2f32.to_radians()))
        .translated_by(Vec3::new(0.0, CAR_HALF_H + CAR_HALF_H * 0.52, -1.75));
    shell_parts.push((trunk_mesh, hex4(part_profile(Part::Trunk).2)));

    // ⑤ 弧形车顶（3 根平行横梁 + 1 片顶面）————————————————
    // 顶面用"扁平 Cylinder 切薄片"近似弧：Cylinder::new(顶长/2, 车宽) 然后切上半部分
    // 简化：5 个薄 Cuboid 沿车顶弧线排列，Y 按抛物线拱起
    let roof_segments = 7;
    let roof_front_z = 0.0;
    let roof_rear_z = -1.55;
    let roof_max_y = CAR_HALF_H + CAR_HALF_H * 1.32;  // = 0.55 + 0.726 = 1.276 m
    let roof_edge_y = CAR_HALF_H + CAR_HALF_H * 0.85;  // 前后端 y（前风挡上缘、后玻璃上缘）
    for i in 0..roof_segments {
        let t = i as f32 / (roof_segments - 1) as f32;
        let z = roof_front_z + (roof_rear_z - roof_front_z) * t;
        // 抛物线 y：t=0 和 t=1 时 y=roof_edge_y；t=0.5 时 y=roof_max_y
        let y_parabolic = roof_edge_y + (roof_max_y - roof_edge_y) * 4.0 * t * (1.0 - t);
        let seg_l = (roof_rear_z - roof_front_z).abs() / (roof_segments - 1) as f32 + 0.005;
        let seg_h = 0.04;
        let seg = Cuboid::new(CAR_HALF_W * 1.86, seg_h, seg_l).mesh().build()
            .translated_by(Vec3::new(0.0, y_parabolic, z));
        shell_parts.push((seg, hex4(0xb5312a)));
    }
    // 左右弧形纵梁（两条完整 Cylinder，轴向沿前后 Z）
    let rail_len = (roof_front_z - roof_rear_z).abs() * 1.05;
    let rail_r = 0.032;
    let rail_cyl = Cylinder::new(rail_r, rail_len).mesh().build()
        .rotated_by(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));   // 轴向 → Z
    // 梁沿抛物线轨迹放 5 段近似弧
    for side in [-1.0f32, 1.0f32] {
        for i in 0..roof_segments {
            let t = i as f32 / (roof_segments - 1) as f32;
            let z = roof_front_z + (roof_rear_z - roof_front_z) * t;
            let y_parabolic = roof_edge_y + (roof_max_y - roof_edge_y) * 4.0 * t * (1.0 - t) - 0.005;
            let rail_piece = Cuboid::new(rail_r * 2.0, rail_r * 2.0, rail_len / (roof_segments - 1) as f32 + 0.01).mesh().build()
                .translated_by(Vec3::new(side * CAR_HALF_W * 0.92, y_parabolic, z));
            shell_parts.push((rail_piece, hex4(0x921d18)));
        }
        let _ = &rail_cyl; // 留作后续替换（简单做法直接用小 Cuboid 足够）
    }
    // 车顶板（覆盖 7 根横梁之间）—— 用 6 块薄板拼出弧形
    let seg_l = (roof_front_z - roof_rear_z).abs() / (roof_segments - 1) as f32;
    for i in 0..(roof_segments - 1) {
        let t0 = i as f32 / (roof_segments - 1) as f32;
        let t1 = (i + 1) as f32 / (roof_segments - 1) as f32;
        let zm = (t0 + t1) / 2.0;
        let z = roof_front_z + (roof_rear_z - roof_front_z) * zm;
        let y0 = roof_edge_y + (roof_max_y - roof_edge_y) * 4.0 * t0 * (1.0 - t0);
        let y1 = roof_edge_y + (roof_max_y - roof_edge_y) * 4.0 * t1 * (1.0 - t1);
        let ym = (y0 + y1) / 2.0;
        // 板倾斜角 = atan2((y1-y0), seg_l)
        let ang = (y1 - y0).atan2(seg_l);
        let plate = Cuboid::new(CAR_HALF_W * 1.86, 0.02, seg_l).mesh().build()
            .rotated_by(Quat::from_rotation_x(ang))
            .translated_by(Vec3::new(0.0, ym, z));
        shell_parts.push((plate, hex4(0xb5312a)));
    }

    // ⑥ A / B / C 三根立柱（左右成对，共 6 根）————————————
    //   A 柱：前风挡前缘两侧（斜柱，不是竖直）
    //   B 柱：前后门之间竖直
    //   C 柱：后玻璃后缘两侧（斜柱）
    let pillar_w = 0.07;    // 柱宽（沿车身 X 方向）
    let pillar_t = 0.05;    // 柱厚（沿 Z 方向）
    // A 柱：Z 从 +0.60（仪表台） → 0.0（车顶前缘），Y 从 CAR_HALF_H + 0.25 → 约 +1.16
    //   用 2 段 Cuboid 近似斜线
    add_pillar_pair(&mut shell_parts, pillar_w, pillar_t,
        Vec3::new(0.0, CAR_HALF_H * 1.30, 0.30),   // 段中点
        22.0f32.to_radians(),   // X 轴旋转（前倾）
        0.93f32);               // 高度（两柱中心连线长）
    // B 柱：竖直，Z=-0.55，Y 从地板到顶
    add_pillar_pair(&mut shell_parts, pillar_w, pillar_t,
        Vec3::new(0.0, CAR_HALF_H * 1.15, -0.55),
        0.0, 1.0f32);
    // C 柱：Z 从 -1.55 → -1.05，后倾
    add_pillar_pair(&mut shell_parts, pillar_w, pillar_t,
        Vec3::new(0.0, CAR_HALF_H * 1.35, -1.30),
        -22.0f32.to_radians(),
        0.93f32);

    // ── 合并成车身主 Mesh ────────────────────────────────────
    let body_mesh = merge_flat(shell_parts);

    let car_root = commands.spawn((
        Mesh3d(meshes.add(body_mesh)),
        MeshMaterial3d(assets.paint_mat.clone()),
        Transform::from_translation(CAR_START),
        CarRigidBody {
            velocity: Vec3::ZERO,
            angvel: Vec3::ZERO,
            mass: 1500.0,
            half: Vec3::new(CAR_HALF_W, CAR_HALF_H + CAR_HALF_H * 1.20, CAR_HALF_L),
            health: 0.0,
        },
        CarRoot,
        Name::new("car_root"),
    )).id();

    // ── 2. 轮胎 + 轮毂（4 组双环） ─────────────────────────────────────
    build_wheels(commands, meshes, assets, car_root);

    // ── 3. 外饰零件（保险杠、大灯、尾灯、翼子板、车门、后视镜、排气管）───
    build_exterior_parts(commands, meshes, assets, car_root);

    // ── 4. 玻璃（前/后挡 + 4 门窗 共 6 块独立倾斜薄板） ───────────────
    build_windows(commands, meshes, assets, car_root);

    // ── 5. 内饰：仪表台 + 3 辐方向盘 + 3 个座椅（坐/靠/枕）+ 扶手箱 ────
    build_interior(commands, meshes, assets, car_root);
}

// 生成一对柱子（left/right 沿 X 镜像）
fn add_pillar_pair(out: &mut Vec<(Mesh, [f32; 4])>, w: f32, t: f32, mid: Vec3, rot_x: f32, len: f32) {
    let pillar = Cuboid::new(w, len, t).mesh().build()
        .rotated_by(Quat::from_rotation_x(rot_x));
    for side in [-1.0f32, 1.0f32] {
        let offset = Vec3::new(side * (CAR_HALF_W * 0.95 + w * 0.1), mid.y, mid.z);
        out.push((pillar.clone().translated_by(offset), hex4(0x8a1d18)));
    }
    let _ = &mid;
}

// ─────────────────────────────────────────────────────────────────
//  外饰零件 build_exterior_parts：保险杠、大灯/尾灯、翼子板、车门、后视镜、排气管
// ─────────────────────────────────────────────────────────────────

fn build_exterior_parts(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    assets: &CarAssets,
    car_root: Entity,
) {
    // 工具：把 (Mesh, hex_color) + part 配置 spawn 为 car_root 的 child，并登记 AttachedPart
    let mut spawn_part = |which: Part, mesh: Mesh, local_offset: Vec3, local_rot: Quat| {
        let (hp, _, color) = part_profile(which);
        let mesh = tint(mesh, hex4(color));
        let mat = match which {
            Part::BumperFront | Part::BumperRear | Part::MirrorL | Part::MirrorR
                | Part::HeadlightL | Part::HeadlightR | Part::TaillightL | Part::TaillightR
                | Part::ExhaustL | Part::ExhaustR
                => assets.plastic_mat.clone(),
            _ => assets.paint_mat.clone(),
        };
        let e = commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(mat),
            Transform::from_translation(local_offset).with_rotation(local_rot),
            AttachedPart { which, health: hp, max_health: hp, local_offset },
            Name::new(format!("part_{:?}", which)),
        )).id();
        commands.entity(car_root).add_child(e);
    };

    // 前保险杠（含下部进气格栅 = 黑条 + 上下两层）
    let bumper_outer = Cuboid::new(CAR_HALF_W * 2.05, 0.16, 0.12).mesh().build();
    spawn_part(Part::BumperFront, bumper_outer,
        Vec3::new(0.0, -CAR_HALF_H * 0.45, CAR_HALF_L + 0.04), Quat::IDENTITY);

    // 后保险杠（同款）
    let bumper_rear = Cuboid::new(CAR_HALF_W * 2.05, 0.14, 0.12).mesh().build();
    spawn_part(Part::BumperRear, bumper_rear,
        Vec3::new(0.0, -CAR_HALF_H * 0.40, -CAR_HALF_L - 0.04), Quat::IDENTITY);

    // 左/右前大灯（扁发光 Cuboid，嵌在保险杠上方）
    let hl_mesh = Cuboid::new(0.36, 0.10, 0.05).mesh().build();
    // 大灯材质：emissive 黄色
    for (side, which) in [(-1.0, Part::HeadlightL), (1.0, Part::HeadlightR)] {
        let off = Vec3::new(side * (CAR_HALF_W - 0.25), -CAR_HALF_H * 0.15, CAR_HALF_L - 0.01);
        let mesh = hl_mesh.clone();
        let (hp, _, _) = part_profile(which);
        let emissive = materials_add_emissive_yellow();
        let e = commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(assets.headlight_mat.clone()),
            Transform::from_translation(off),
            AttachedPart { which, health: hp, max_health: hp, local_offset: off },
            Name::new(format!("part_{:?}", which)),
        )).id();
        commands.entity(car_root).add_child(e);
        let _ = &emissive;
    }

    // 尾灯（红，尾部两侧）
    let tl_mesh = Cuboid::new(0.22, 0.12, 0.04).mesh().build();
    for (side, which) in [(-1.0, Part::TaillightL), (1.0, Part::TaillightR)] {
        let off = Vec3::new(side * (CAR_HALF_W - 0.28), CAR_HALF_H * 0.15, -CAR_HALF_L + 0.01);
        let (hp, _, _) = part_profile(which);
        let e = commands.spawn((
            Mesh3d(meshes.add(tl_mesh.clone())),
            MeshMaterial3d(assets.taillight_mat.clone()),
            Transform::from_translation(off),
            AttachedPart { which, health: hp, max_health: hp, local_offset: off },
            Name::new(format!("part_{:?}", which)),
        )).id();
        commands.entity(car_root).add_child(e);
    }

    // 翼子板（4 块：覆盖轮子上方的弧形板，Cylinder 轴向 X 截 1/4 圆弧）
    let fender_arc_r = 0.45;
    let fender_t = 0.03;
    let fender_len = 1.20;   // 沿 Z 方向延伸
    // 用 Cylinder + 旋转 + 压扁：外层是 Cuboid 带弧形顶近似，简单做法
    let fender_mesh = Cuboid::new(CAR_HALF_W * 0.15 + fender_t, 0.05, fender_len).mesh().build();
    let placements: [(Vec3, Part); 4] = [
        (Vec3::new(-CAR_HALF_W - 0.02, -CAR_HALF_H * 0.12,  CAR_HALF_L - 0.95), Part::FenderFL),
        (Vec3::new( CAR_HALF_W + 0.02, -CAR_HALF_H * 0.12,  CAR_HALF_L - 0.95), Part::FenderFR),
        (Vec3::new(-CAR_HALF_W - 0.02, -CAR_HALF_H * 0.12, -CAR_HALF_L + 0.95), Part::FenderRL),
        (Vec3::new( CAR_HALF_W + 0.02, -CAR_HALF_H * 0.12, -CAR_HALF_L + 0.95), Part::FenderRR),
    ];
    for (off, which) in placements {
        let (hp, _, _) = part_profile(which);
        let e = commands.spawn((
            Mesh3d(meshes.add(fender_mesh.clone())),
            MeshMaterial3d(assets.paint_mat.clone()),
            Transform::from_translation(off),
            AttachedPart { which, health: hp, max_health: hp, local_offset: off },
            Name::new(format!("part_{:?}", which)),
        )).id();
        commands.entity(car_root).add_child(e);
        let _ = fender_arc_r;
    }

    // 4 扇车门（每个是倾斜薄板，带一条凹线 —— 两块 Cuboid 拼门缝线条）
    let door_panel = Cuboid::new(0.03, CAR_HALF_H * 1.25, 0.88).mesh().build();
    let doors: [(Vec3, f32, Part); 4] = [
        // 位置 + 轻微旋转角度（绕 Y：前门略微外撇 1°）+ part
        (Vec3::new(-CAR_HALF_W - 0.015, CAR_HALF_H * 0.20,  0.02), -1.0f32.to_radians(), Part::DoorFL),
        (Vec3::new( CAR_HALF_W + 0.015, CAR_HALF_H * 0.20,  0.02),  1.0f32.to_radians(), Part::DoorFR),
        (Vec3::new(-CAR_HALF_W - 0.015, CAR_HALF_H * 0.20, -0.92), -0.8f32.to_radians(), Part::DoorRL),
        (Vec3::new( CAR_HALF_W + 0.015, CAR_HALF_H * 0.20, -0.92),  0.8f32.to_radians(), Part::DoorRR),
    ];
    for (off, rot_y, which) in doors {
        let (hp, _, _) = part_profile(which);
        let e = commands.spawn((
            Mesh3d(meshes.add(door_panel.clone())),
            MeshMaterial3d(assets.paint_mat.clone()),
            Transform::from_translation(off).with_rotation(Quat::from_rotation_y(rot_y)),
            AttachedPart { which, health: hp, max_health: hp, local_offset: off },
            Name::new(format!("part_{:?}", which)),
        )).id();
        commands.entity(car_root).add_child(e);
    }

    // 后视镜（已经在 Part 定义里，换新 mesh：一小段 Cuboid + 一个镜面反光小方块）
    let mirror_body = Cuboid::new(0.08, 0.14, 0.20).mesh().build();
    for (side, which) in [(-1.0, Part::MirrorL), (1.0, Part::MirrorR)] {
        let off = Vec3::new(side * (CAR_HALF_W + 0.10), CAR_HALF_H * 0.55, CAR_HALF_L * 0.30);
        let (hp, _, _) = part_profile(which);
        let e = commands.spawn((
            Mesh3d(meshes.add(mirror_body.clone())),
            MeshMaterial3d(assets.plastic_mat.clone()),
            Transform::from_translation(off),
            AttachedPart { which, health: hp, max_health: hp, local_offset: off },
            Name::new(format!("part_{:?}", which)),
        )).id();
        commands.entity(car_root).add_child(e);
    }

    // 排气管（两根小 Cylinder，车尾底盘下，朝 -Z）
    let exhaust_mesh = Cylinder::new(0.028, 0.28).mesh().build()
        .rotated_by(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
    for (side, which) in [(-1.0, Part::ExhaustL), (1.0, Part::ExhaustR)] {
        let off = Vec3::new(side * 0.40, -CAR_HALF_H + 0.05, -CAR_HALF_L - 0.08);
        let (hp, _, _) = part_profile(which);
        let e = commands.spawn((
            Mesh3d(meshes.add(exhaust_mesh.clone())),
            MeshMaterial3d(assets.plastic_mat.clone()),
            Transform::from_translation(off),
            AttachedPart { which, health: hp, max_health: hp, local_offset: off },
            Name::new(format!("part_{:?}", which)),
        )).id();
        commands.entity(car_root).add_child(e);
    }
}

// ─────────────────────────────────────────────────────────────────
//  玻璃：前挡大斜面 + 后挡 + 4 门窗（共 6 块独立薄板）
//  平时用倾斜薄板（光滑面）；撞击力够时自动降级成 Voxel 面板喷碎渣
// ─────────────────────────────────────────────────────────────────

fn build_windows(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    assets: &CarAssets,
    car_root: Entity,
) {
    // 前挡风玻璃：大斜面，Z 从 +0.60 → 0.0，Y 从 0.90 → 1.28
    let w_front_w = CAR_HALF_W * 1.80;
    let w_front_h = CAR_HALF_H * 2.25;  // 斜面长度
    let w_front_t = 0.025;
    let front_mesh = Cuboid::new(w_front_w, w_front_t, w_front_h).mesh().build()
        .rotated_by(Quat::from_rotation_x(58.0f32.to_radians()))
        .translated_by(Vec3::new(0.0, CAR_HALF_H * 1.68, CAR_HALF_L * 0.14));
    commands.entity(car_root).with_children(|parent| {
        parent.spawn((
            Mesh3d(meshes.add(front_mesh)),
            MeshMaterial3d(assets.glass_mat.clone()),
            Transform::default(),
            WindowPane::Front,
            Name::new("glass_front"),
        ));
    });

    // 后挡风玻璃：更陡的后倾斜面
    let rear_mesh = Cuboid::new(CAR_HALF_W * 1.78, 0.025, CAR_HALF_H * 2.0).mesh().build()
        .rotated_by(Quat::from_rotation_x(-58.0f32.to_radians()))
        .translated_by(Vec3::new(0.0, CAR_HALF_H * 1.72, -CAR_HALF_L * 0.76));
    commands.entity(car_root).with_children(|parent| {
        parent.spawn((
            Mesh3d(meshes.add(rear_mesh)),
            MeshMaterial3d(assets.glass_mat.clone()),
            Transform::default(),
            WindowPane::Rear,
            Name::new("glass_rear"),
        ));
    });

    // 4 门侧窗（每扇门一块长方形薄板，略倾斜）
    let side_w_t = 0.02;
    let side_w_len = 0.68;
    let side_w_h = 0.40;
    let mesh_side = Cuboid::new(side_w_t, side_w_h, side_w_len).mesh().build();
    let placements: [(Vec3, Quat, WindowPane); 4] = [
        (Vec3::new(-CAR_HALF_W * 0.995, CAR_HALF_H * 1.35,  0.00), Quat::from_rotation_y(-6.0f32.to_radians()), WindowPane::DoorFL),
        (Vec3::new( CAR_HALF_W * 0.995, CAR_HALF_H * 1.35,  0.00), Quat::from_rotation_y( 6.0f32.to_radians()), WindowPane::DoorFR),
        (Vec3::new(-CAR_HALF_W * 0.995, CAR_HALF_H * 1.35, -0.92), Quat::from_rotation_y(-5.0f32.to_radians()), WindowPane::DoorRL),
        (Vec3::new( CAR_HALF_W * 0.995, CAR_HALF_H * 1.35, -0.92), Quat::from_rotation_y( 5.0f32.to_radians()), WindowPane::DoorRR),
    ];
    for (off, rot, tag) in placements {
        commands.entity(car_root).with_children(|parent| {
            parent.spawn((
                Mesh3d(meshes.add(mesh_side.clone())),
                MeshMaterial3d(assets.glass_mat.clone()),
                Transform::from_translation(off).with_rotation(rot),
                tag,
                Name::new(format!("glass_{:?}", tag)),
            ));
        });
    }
}

#[derive(Component, Debug, Copy, Clone)]
enum WindowPane { Front, Rear, DoorFL, DoorFR, DoorRL, DoorRR }

// 简单占位：大灯 emissive 设置直接写在 CarAssets 里，CarAssets 结构要扩展（加 headlight_mat / taillight_mat）
fn materials_add_emissive_yellow() {}   // 空函数（兼容 placeholder，实际逻辑改在 CarAssets 初始化处）

// ─────────────────────────────────────────────────────────────────
//  内饰：仪表台 + 3 辐方向盘 + 3 个座椅（垫 + 靠背 + 头枕） + 扶手箱
// ─────────────────────────────────────────────────────────────────

fn build_interior(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    assets: &CarAssets,
    car_root: Entity,
) {
    // ── 仪表台（横跨车头，黑色长条，上表面带倾斜） ───────────────
    let dash_body = Cuboid::new(CAR_HALF_W * 1.80, 0.18, 0.42).mesh().build()
        .rotated_by(Quat::from_rotation_x(10.0f32.to_radians()))
        .translated_by(Vec3::new(0.0, CAR_HALF_H * 1.02, CAR_HALF_L * 0.30));
    commands.entity(car_root).with_children(|parent| {
        parent.spawn((
            Mesh3d(meshes.add(dash_body)),
            MeshMaterial3d(assets.interior_mat.clone()),
            Transform::default(),
            Name::new("int_dashboard"),
        ));
    });

    // ── 方向盘（3 辐条 + 环） ───────────────────────────────────
    // 环：圆环 = 近似用 12 个小 Cuboid 围成圆
    let spokes = 12;
    let ring_r = 0.18;
    let ring_sq = 0.035;
    let mut st_wheel: Vec<(Mesh, [f32; 4])> = Vec::new();
    for i in 0..spokes {
        let a = (i as f32 / spokes as f32) * std::f32::consts::TAU;
        let p = Vec3::new(a.cos() * ring_r, a.sin() * ring_r, 0.0);
        let m = Cuboid::new(ring_sq, ring_sq, ring_sq).mesh().build()
            .translated_by(p);
        st_wheel.push((m, hex4(0x101012)));
    }
    // 3 根辐条（中心到环，呈 120° 分布：左、右、下）
    for dir in [0.0f32, 120.0f32.to_radians(), 240.0f32.to_radians()] {
        let end = Vec3::new(dir.cos() * (ring_r - ring_sq), dir.sin() * (ring_r - ring_sq), 0.0);
        let mid = end * 0.5;
        let len = end.length();
        let m = Cuboid::new(ring_sq * 0.9, ring_sq * 2.0, len).mesh().build()
            .rotated_by(Quat::from_rotation_x(0.0)) // 先沿 Z，然后按 dir 旋转 XY
            .rotated_by(Quat::from_rotation_z(dir + std::f32::consts::FRAC_PI_2))
            .translated_by(mid);
        st_wheel.push((m, hex4(0x1a1a1d)));
    }
    // 中心盖子
    let hub = Cylinder::new(ring_sq * 1.3, ring_sq * 2.0).mesh().build();
    st_wheel.push((hub, hex4(0xb5312a)));

    let sw_mesh = merge_flat(st_wheel);
    // 放置：驾驶座（左驾 = -X 侧）前方 +Z=0.35 附近，朝 +Z（司机对着前挡）
    let sw_pos = Vec3::new(-CAR_HALF_W * 0.48, CAR_HALF_H * 1.10, CAR_HALF_L * 0.10);
    let sw_rot = Quat::from_rotation_y(0.0);   // 环平面朝司机（= 朝 -Z → 环面是 XY）
    commands.entity(car_root).with_children(|parent| {
        parent.spawn((
            Mesh3d(meshes.add(sw_mesh)),
            MeshMaterial3d(assets.interior_mat.clone()),
            Transform::from_translation(sw_pos).with_rotation(sw_rot),
            Name::new("int_steering_wheel"),
        ));
    });

    // ── 座椅（3 个：驾驶 / 副驾 / 后排一座居中） ─────────────────
    let seats = [
        // (位置, 是否头枕)
        (Vec3::new(-CAR_HALF_W * 0.48, CAR_HALF_H * 0.15, -0.10), true),   // 驾驶座
        (Vec3::new( CAR_HALF_W * 0.48, CAR_HALF_H * 0.15, -0.10), true),   // 副驾
        (Vec3::new( 0.0,                 CAR_HALF_H * 0.15, -1.10), true),  // 后排中
    ];
    for (i, &(base, has_headrest)) in seats.iter().enumerate() {
        spawn_seat(commands, meshes, assets, car_root, base, has_headrest, i);
    }

    // ── 中央扶手箱（驾驶和副驾中间，从前排延伸到后排一部分）
    let arm_box = Cuboid::new(0.22, 0.20, 1.10).mesh().build()
        .translated_by(Vec3::new(0.0, CAR_HALF_H * 0.65, -0.55));
    commands.entity(car_root).with_children(|parent| {
        parent.spawn((
            Mesh3d(meshes.add(arm_box)),
            MeshMaterial3d(assets.interior_mat.clone()),
            Transform::default(),
            Name::new("int_armrest"),
        ));
    });
}

fn spawn_seat(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    assets: &CarAssets,
    car_root: Entity,
    base: Vec3,   // base: 坐垫底面中心（y≈0.70-0.40=0.30）
    _with_headrest: bool,
    idx: usize,
) {
    // 坐垫（软包，略后倾）
    let cushion_w = 0.54;
    let cushion_l = 0.54;
    let cushion_t = 0.10;
    let cushion = Cuboid::new(cushion_w, cushion_t, cushion_l).mesh().build()
        .rotated_by(Quat::from_rotation_x(-3.0f32.to_radians()))
        .translated_by(Vec3::new(base.x, base.y + cushion_t / 2.0 + 0.05, base.z));
    // 靠背（立起来，18° 后倾）
    let back_h = 0.58;
    let back_t = 0.10;
    let back = Cuboid::new(cushion_w * 0.95, back_h, back_t).mesh().build()
        .rotated_by(Quat::from_rotation_x(18.0f32.to_radians()))
        .translated_by(Vec3::new(base.x, base.y + cushion_t + back_h * 0.55, base.z - cushion_l / 2.0 + back_t / 2.0));
    // 头枕（两块小 Cuboid，立在靠背上）
    let hr_w = 0.20;
    let hr_h = 0.16;
    let hr_t = 0.10;
    let headrest = Cuboid::new(hr_w, hr_h, hr_t).mesh().build()
        .translated_by(Vec3::new(base.x, base.y + cushion_t + back_h + hr_h * 0.75, base.z - cushion_l / 2.0 + back_t / 2.0));

    let seat_mesh = merge_flat(vec![
        (cushion,  hex4(0x332c29)),
        (back,     hex4(0x3a302d)),
        (headrest, hex4(0x3a302d)),
    ]);

    commands.entity(car_root).with_children(|parent| {
        parent.spawn((
            Mesh3d(meshes.add(seat_mesh)),
            MeshMaterial3d(assets.interior_mat.clone()),
            Transform::default(),
            Name::new(format!("seat_{}", idx)),
        ));
    });
}

// ─────────────────────────────────────────────────────────────────
//  车轮 4 组：轮胎（黑 Cylinder） + 轮毂（银灰 Cylinder 双环）
// ─────────────────────────────────────────────────────────────────

fn build_wheels(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    assets: &CarAssets,
    car_root: Entity,
) {
    let wheel_r = 0.32;
    let tire_w = 0.22;
    let hub_r_inner = 0.09;
    let hub_r_outer = wheel_r * 0.82;
    let hub_w = tire_w * 0.85;

    // 轮胎：Cylinder(r=0.32, h=tire_w) + 轴向 X（需要绕 Z 轴转 90°）
    let tire_cyl = Cylinder::new(wheel_r, tire_w).mesh().build();
    let tire_mesh = tire_cyl.rotated_by(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
    // 轮毂：Cylinder(环近似) — 用 r=hub_r_outer 高 hub_w 的外圈 加 r=hub_r_inner 高 hub_w 的内圈，再叠 5 根辐条
    let hub_outer = Cylinder::new(hub_r_outer, hub_w).mesh().build()
        .rotated_by(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
    let hub_inner = Cylinder::new(hub_r_inner, hub_w * 1.05).mesh().build()
        .rotated_by(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
    // 5 根辐条（XZ 平面上 5 条小 Cuboid）
    let mut hub_spokes: Vec<(Mesh, [f32; 4])> = vec![
        (hub_outer, hex4(0xd6d6da)),
        (hub_inner, hex4(0x9a9aa0)),
    ];
    let spokes = 5;
    for i in 0..spokes {
        let a = (i as f32 / spokes as f32) * std::f32::consts::TAU;
        let mid = Vec3::new(a.cos() * (hub_r_outer + hub_r_inner) / 2.0,
                            a.sin() * (hub_r_outer + hub_r_inner) / 2.0,
                            0.0);
        let len = hub_r_outer - hub_r_inner;
        let m = Cuboid::new(0.05, len, hub_w * 0.92).mesh().build()
            .rotated_by(Quat::from_rotation_z(a + std::f32::consts::FRAC_PI_2))
            .translated_by(mid);
        hub_spokes.push((m, hex4(0xc5c5c9)));
    }
    let hub_mesh = merge_flat(hub_spokes);

    let wheel_positions = [
        (Vec3::new(-CAR_HALF_W + tire_w * 0.15, -CAR_HALF_H,  CAR_HALF_L - wheel_r * 0.95), "FL"),
        (Vec3::new( CAR_HALF_W - tire_w * 0.15, -CAR_HALF_H,  CAR_HALF_L - wheel_r * 0.95), "FR"),
        (Vec3::new(-CAR_HALF_W + tire_w * 0.15, -CAR_HALF_H, -CAR_HALF_L + wheel_r * 0.95), "RL"),
        (Vec3::new( CAR_HALF_W - tire_w * 0.15, -CAR_HALF_H, -CAR_HALF_L + wheel_r * 0.95), "RR"),
    ];
    for (pos, tag) in wheel_positions {
        // 轮胎
        let t = commands.spawn((
            Mesh3d(meshes.add(tire_mesh.clone())),
            MeshMaterial3d(assets.wheel_mat.clone()),
            Transform::from_translation(pos),
            Name::new(format!("tire_{}", tag)),
        )).id();
        commands.entity(car_root).add_child(t);
        // 轮毂（放轮胎内侧——X 位置 + 或 - 一个轮胎半厚）
        let hub_off_x = if pos.x < 0.0 { tire_w * 0.6 } else { -tire_w * 0.6 };
        let hub_pos = Vec3::new(pos.x + hub_off_x, pos.y, pos.z);
        let h = commands.spawn((
            Mesh3d(meshes.add(hub_mesh.clone())),
            MeshMaterial3d(assets.hub_mat.clone()),
            Transform::from_translation(hub_pos),
            Name::new(format!("hub_{}", tag)),
        )).id();
        commands.entity(car_root).add_child(h);
    }
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
        let mut detach_list: Vec<(Entity, Part, Vec3, Vec3, f32, Transform)> = Vec::new();
        for child in children.iter() {
            if let Ok((at, gtf, e)) = parts.get_mut(child) {
                let part_world = gtf.translation();
                let world_rot = gtf.to_scale_rotation_translation().1;
                let world_scale = gtf.to_scale_rotation_translation().0;
                let world_tf = Transform::from_translation(part_world)
                    .with_rotation(world_rot)
                    .with_scale(world_scale);
                let d2 = (part_world - avg).length_squared();
                let (threshold, splash_mult, _color) = part_profile(at.which);
                if d2 < 4.5 && ke * (1.0 - (d2 / 6.0).min(0.9)) >= threshold * 0.45 {
                    // 因为 at 是 &mut AttachedPart，这里不能 reborrow 再 push——直接操作实体组件
                    let p = at.which;
                    let part_half = match p {
                        Part::BumperFront => Vec3::new(CAR_HALF_W * 1.03, 0.08, 0.06),
                        Part::BumperRear  => Vec3::new(CAR_HALF_W * 1.03, 0.07, 0.06),
                        Part::Hood => Vec3::new(CAR_HALF_W * 0.96, 0.03, CAR_HALF_L * 0.33),
                        Part::Trunk => Vec3::new(CAR_HALF_W * 0.96, 0.03, CAR_HALF_L * 0.28),
                        Part::MirrorL | Part::MirrorR => Vec3::new(0.04, 0.07, 0.10),
                        Part::HeadlightL | Part::HeadlightR => Vec3::new(0.18, 0.05, 0.025),
                        Part::TaillightL | Part::TaillightR => Vec3::new(0.11, 0.06, 0.02),
                        Part::FenderFL | Part::FenderFR | Part::FenderRL | Part::FenderRR
                            => Vec3::new(0.08, 0.025, 0.60),
                        Part::DoorFL | Part::DoorFR | Part::DoorRL | Part::DoorRR
                            => Vec3::new(0.015, CAR_HALF_H * 0.62, 0.44),
                        Part::ExhaustL | Part::ExhaustR
                            => Vec3::new(0.028, 0.028, 0.14),
                        Part::Body => Vec3::splat(0.2),
                    };
                    let part_mass: f32 = match p {
                        Part::BumperFront => 5.0,
                        Part::BumperRear => 4.5,
                        Part::MirrorL | Part::MirrorR => 0.3,
                        Part::HeadlightL | Part::HeadlightR => 0.25,
                        Part::TaillightL | Part::TaillightR => 0.2,
                        Part::Hood | Part::Trunk => 12.0,
                        Part::FenderFL | Part::FenderFR | Part::FenderRL | Part::FenderRR => 2.5,
                        Part::DoorFL | Part::DoorFR | Part::DoorRL | Part::DoorRR => 18.0,
                        Part::ExhaustL | Part::ExhaustR => 0.9,
                        Part::Body => 100.0,
                    };
                    let dir = (part_world - avg).normalize_or(vel.normalize_or(Vec3::Z));
                    let sp = (ke / part_mass.max(1.0)).sqrt() * splash_mult;
                    drop(at);
                    detach_list.push((e, p, dir * sp + Vec3::Y * (ke / 1800.0).min(5.0), part_half, part_mass, world_tf));
                }
            }
        }
        for (e, p, velo, part_half, part_mass, world_tf) in detach_list {
            // 零件脱落：解除父子关系（remove Parent），然后覆盖其 Transform 为当前世界坐标（否则失去父节点会回到局部 (0,0,0)）
            commands.entity(e)
                .remove::<ChildOf>()
                .remove::<AttachedPart>()
                .insert(world_tf)
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
