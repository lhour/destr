//! 可破坏砖墙 —— 应用骨架（第 2、3、4 步）。
//!
//! 【第 2 步】setup：相机、灯光、地面、按 chunk 生成墙实体
//! 【第 3 步】punch_on_click：鼠标点击 → 射线 → 命中砖 → 改数据 → 只重建该 chunk
//! 【第 4 步】Debris + debris_physics：被打掉的砖变成临时飞行实体，落地弹跳后冻结
//!
//! 交互：左键点墙打砖。运行 `cargo run -- --demo` 可无人自动演示（打洞+截图+退出）。

mod wall;

use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::render::view::Hdr;
use bevy::window::{PrimaryWindow, WindowResolution};

fn main() {
    // --demo：无人值守模式（沙箱里自动验证用；本地玩不用加）
    let demo = std::env::args().any(|a| a == "--demo");

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Destructible Wall — 左键打砖".into(),
            resolution: WindowResolution::new(1280, 720),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.68, 0.80, 0.92)))
    .insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.62, 0.68, 0.8),
        brightness: 900.0,
        affects_lightmapped_meshes: true,
    })
    // 第 1 步的数据层在这里进入 ECS（一面墙只是一个 Resource！）
    .insert_resource(wall::WallData::new())
    .insert_resource(DemoState::default())
    .add_message::<BlockPunched>() // 注册消息（发件箱/收件箱）
    .add_systems(Startup, setup)
    .add_systems(Update, (punch_on_click, apply_punch, debris_physics));

    if demo {
        // 演示模式：按时间表"假装点击" + 截图 + 退出
        app.add_systems(Update, (demo_punch, demo_screenshot, demo_exit));
    }
    app.run();
}

/// "某块砖被打掉了"消息 —— 输入层（鼠标/演示/将来 NPC/爆炸）与执行层解耦。
/// 为什么要消息：MeshRayCast 内部占用 Res<Assets<Mesh>>（只读），
/// 执行破坏又需要 ResMut<Assets<Mesh>>（读写）—— 同一系统里两者冲突（错误 B0002）。
/// 拆成两个系统 + 一条消息，各拿各的访问权，Bevy 自动安排执行顺序。
#[derive(Message)]
struct BlockPunched {
    c: usize,
    y: usize,
    x: usize,
}

// ── 共享资源 ───────────────────────────────────────────────────────

/// 全场景共享的材质 + 碎砖网格（合批的关键：一个白色材质 + 顶点色）
#[derive(Resource)]
struct WallAssets {
    material: Handle<StandardMaterial>,
    debris_mesh: Handle<Mesh>, // 小方块，所有碎砖共用（Instancing 级合批）
}

/// 演示模式状态
#[derive(Resource, Default)]
struct DemoState {
    punched: usize,
    shot_taken: bool,
}

/// 标记一块墙 chunk 实体：它负责表格里的哪片区域
#[derive(Component)]
struct WallChunk {
    cx: usize,
    cy: usize,
}

// ── 第 2 步：场景搭建 ──────────────────────────────────────────────

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    wall: Res<wall::WallData>,
) {
    // 砖面贴图（程序化生成：边框 + 砂浆 + 噪点，零外部资源，见 wall.rs）
    let brick_tex = images.add(wall::brick_texture());
    // 砖的材质：白底 × 贴图。每砖颜色仍由顶点色提供（合批不受影响，
    // 因为整面墙还是共享这 1 个材质）
    let brick_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(brick_tex),
        perceptual_roughness: 0.95,
        ..default()
    });
    // 素面白材质（地面用，不带贴图）
    let plain_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        ..default()
    });
    // 碎砖：小方块 + 中档石色顶点色。材质带砖面贴图 → 碎块自带"断砖"质感
    let mut debris_mesh = Cuboid::new(0.22, 0.22, 0.22).mesh().build();
    let n = debris_mesh.count_vertices();
    debris_mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.62, 0.60, 0.55, 1.0]; n]);
    let debris_mesh = meshes.add(debris_mesh);
    commands.insert_resource(WallAssets { material: brick_mat.clone(), debris_mesh });

    // 暖阳光 + 阴影（无阴影 = 漂浮感）
    commands.spawn((
        DirectionalLight {
            illuminance: 40_000.0,
            color: Color::srgb(1.0, 0.96, 0.88),
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, -0.6, -0.2)),
    ));

    // 地面（参照物：没有它墙就像飘在虚空里）
    let mut ground = Cuboid::new(60.0, 0.4, 60.0).mesh().build();
    ground = ground.translated_by(Vec3::new(0.0, -0.2, 0.0));
    let n = ground.count_vertices();
    let g = Color::srgb_u8(0x6b, 0x6b, 0x5e).to_linear(); // 压实的土色
    ground.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[g.red, g.green, g.blue, 1.0]; n]);
    ground.duplicate_vertices();
    ground.compute_flat_normals();
    commands.spawn((
        Mesh3d(meshes.add(ground)),
        MeshMaterial3d(plain_mat.clone()),
        Transform::default(),
    ));

    // 墙：每块 chunk 一个实体。3×2 = 6 个实体 = 6 个 draw call 撑起整面墙。
    // 打掉砖只重建其中 1 个 —— 这就是"破坏不卡"的架构。
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

    // 相机
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(9.0, 4.5, 14.0).looking_at(Vec3::new(0.0, 2.0, 0.0), Vec3::Y),
        Hdr,
    ));
}

// ── 第 3 步：点击 → 破坏 ───────────────────────────────────────────

/// 执行破坏的系统：收到 BlockPunched 消息 → 改数据 → 重建 chunk → 生成碎砖。
/// 鼠标、演示、将来的 NPC 挖墙/爆炸，全都只需发一条消息 —— 执行逻辑只写这一份。
fn apply_punch(
    mut punches: MessageReader<BlockPunched>,
    mut wall: ResMut<wall::WallData>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunks: Query<(Entity, &WallChunk, &mut Mesh3d)>,
    assets: Res<WallAssets>,
    mut commands: Commands,
) {
    for p in punches.read() {
        punch_block(&mut *wall, p.c, p.y, p.x, &mut meshes, &mut chunks, &assets, &mut commands);
    }
    // 消息是双缓冲的：本帧写入，下一帧这里读到（1 帧延迟，肉眼不可感知）
}

/// 打掉一块砖的完整流程（数据 → 重建 chunk → 生成碎砖）。
fn punch_block(
    wall: &mut wall::WallData,
    c: usize,
    y: usize,
    x: usize,
    meshes: &mut Assets<Mesh>,
    chunks: &mut Query<(Entity, &WallChunk, &mut Mesh3d)>,
    assets: &WallAssets,
    commands: &mut Commands,
) {
    if !wall.alive(c, y, x) {
        return; // 已经是洞了
    }
    let pos = wall::block_center(c, y, x);

    // 1) 数据层：改一个 bool（破坏的全部本质）
    wall.destroy(c, y, x);

    // 2) 渲染层：只重建这块砖所在的 chunk
    let (cx, cy) = wall::chunk_of(x, y);
    for (_, chunk, mut mesh3d) in chunks.iter_mut() {
        if chunk.cx == cx && chunk.cy == cy {
            **mesh3d = meshes.add(wall::build_chunk_mesh(wall, cx, cy));
            break;
        }
    }

    // 3) 表现层：这块砖"变成"2~3 个飞行碎块（第 4 步）
    spawn_debris(pos, c, y, x, assets, commands);
}

/// 鼠标输入系统：光标 → 相机射线 → 命中墙 → 砖下标 → 发消息。
/// 只做"翻译"，不做破坏 —— 破坏统一在 apply_punch 里执行。
fn punch_on_click(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    chunks: Query<Entity, With<WallChunk>>, // 只用来过滤命中对象
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

    // 射线打到场景里所有网格（墙 chunk + 地面 + 碎砖…）
    let hits = ray_cast.cast_ray(ray, &MeshRayCastSettings::default());

    // 只要"墙 chunk"的命中（过滤掉地面和碎砖）
    let Some((_, hit)) = hits.iter().find(|(e, _)| chunks.get(*e).is_ok()) else {
        return;
    };

    // 命中点在墙【表面】（恰好是格边界），沿法线后退 1cm 进入砖内部，再反算下标
    let inside = hit.point - hit.normal * 0.01;
    let Some((c, y, x)) = wall::block_from_point(inside) else { return };

    punches.write(BlockPunched { c, y, x });
}

// ── 第 4 步：碎砖物理 ──────────────────────────────────────────────

/// 临时飞行碎块。落地弹跳衰减，速度耗尽后移除本组件 = 冻结在原地（碎砖堆）。
#[derive(Component)]
struct Debris {
    velocity: Vec3,
    angvel: Vec3, // 角速度（rad/s），三轴
}

fn spawn_debris(
    pos: Vec3,
    c: usize,
    y: usize,
    x: usize,
    assets: &WallAssets,
    commands: &mut Commands,
) {
    for i in 0..3 {
        let h1 = wall::hash2(x as i32 * 7 + i, y as i32 * 13 + c as i32);
        let h2 = wall::hash2(y as i32 * 5 + i, x as i32 * 11 + c as i32);
        let h3 = wall::hash2(c as i32 * 17 + i, y as i32 * 3 + x as i32);
        commands.spawn((
            Mesh3d(assets.debris_mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            Transform::from_translation(pos + Vec3::new((h1 - 0.5) * 0.5, (h2 - 0.5) * 0.3, (h3 - 0.5) * 0.4))
                .with_scale(Vec3::splat(0.6 + h1 * 0.8)),
            Debris {
                // 向镜头方向（+Z）飞 + 抛起 + 左右散
                velocity: Vec3::new((h1 - 0.5) * 4.0, 2.5 + h2 * 3.0, 1.5 + h3 * 2.5),
                angvel: Vec3::new((h1 - 0.5) * 6.0, (h2 - 0.5) * 6.0, (h3 - 0.5) * 6.0),
            },
        ));
    }
}

/// 手写物理：重力 + 位移 + 旋转 + 地面弹跳。不引物理引擎，30 行讲清原理。
fn debris_physics(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Debris)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut d) in &mut q {
        d.velocity.y -= 15.0 * dt; // 重力
        tf.translation += d.velocity * dt;
        tf.rotate_x(d.angvel.x * dt);
        tf.rotate_y(d.angvel.y * dt);
        tf.rotate_z(d.angvel.z * dt);

        let half = 0.22 * tf.scale.x * 0.5; // 碎块半高
        if tf.translation.y < half && d.velocity.y < 0.0 {
            tf.translation.y = half;
            if d.velocity.y < -3.0 {
                // 还够快：弹一次，衰减
                d.velocity.y = -d.velocity.y * 0.35;
                d.velocity.x *= 0.6;
                d.velocity.z *= 0.6;
                d.angvel *= 0.5;
            } else {
                // 速度耗尽：移除 Debris → 冻结成静态碎砖堆
                commands.entity(e).remove::<Debris>();
            }
        }
        // 出界兜底清理
        if tf.translation.length() > 60.0 {
            commands.entity(e).despawn();
        }
    }
}

// ── 演示模式（沙箱无人验证）───────────────────────────────────────

/// 按时间表"假装点击"打 8 块砖（都打面向相机那皮 c=1；打穿墙要 c=0 也打）。
/// 注意它和 punch_on_click 一样只发消息 —— 演示与真实输入走同一条管道。
fn demo_punch(
    time: Res<Time>,
    mut demo: ResMut<DemoState>,
    mut punches: MessageWriter<BlockPunched>,
) {
    const PUNCHES: [(usize, usize, usize); 8] = [
        (1, 5, 5), (1, 6, 5), (1, 5, 6), (1, 6, 6),
        (1, 4, 6), (1, 7, 5), (1, 4, 5), (1, 7, 6),
    ];
    let t = time.elapsed_secs();
    let i = demo.punched;
    if i < PUNCHES.len() && t >= 0.3 + 0.35 * i as f32 {
        let (c, y, x) = PUNCHES[i];
        punches.write(BlockPunched { c, y, x });
        demo.punched += 1;
    }
}

/// 碎砖落定后（4.2s）截图存盘
fn demo_screenshot(time: Res<Time>, mut demo: ResMut<DemoState>, mut commands: Commands) {
    if time.elapsed_secs() >= 4.2 && !demo.shot_taken {
        demo.shot_taken = true;
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk("destr_shot.png"));
    }
}

fn demo_exit(
    time: Res<Time>,
    wall: Res<wall::WallData>,
    mut exit: MessageWriter<AppExit>,
) {
    if time.elapsed_secs() > 5.0 {
        // 退出前打印 ASCII 截面：控制台里肉眼复核破坏结果（内存截面扫描）
        println!("\n=== 演示结束：最终墙态 ===");
        wall::print_wall_section(&*wall, 1);
        exit.write(AppExit::Success);
    }
}
