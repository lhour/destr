//! 公共场景脚手架：所有 case 都可以复用的灯光/相机/地面/窗口/材质。
//!
//! 这些是"可参考但不必照搬"的示例默认值——case 觉得不适合直接替换即可。

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Mesh, PrimitiveTopology};
use bevy::prelude::*;
use bevy::window::WindowResolution;

/// 默认窗口分辨率（16:9，截图统一尺寸）
pub const WIN_W: u32 = 1280;
pub const WIN_H: u32 = 720;

/// 天空色（淡蓝，避免 case 之间天空风格打架）
pub const SKY: Color = Color::srgb(0.68, 0.80, 0.92);

// ── App 层的一次性配置（放到 main 里调用）──────────────────────────

/// 默认插件 + 共享窗口设置。所有 case 的 `main` 应从这个函数开始。
pub fn add_default_plugins(app: &mut App, title: impl Into<String>) {
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: title.into(),
            resolution: WindowResolution::new(WIN_W, WIN_H),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(SKY))
    .insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.62, 0.68, 0.8),
        brightness: 900.0,
        affects_lightmapped_meshes: true,
    });
}

// ── 场景层公共对象（Startup 里 spawn）───────────────────────────────

/// 暖阳光 + 硬阴影。这是所有室外场景的默认光照。
pub fn spawn_sun(commands: &mut Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 40_000.0,
            color: Color::srgb(1.0, 0.96, 0.88),
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, -0.6, -0.2)),
    ));
}

/// 3/4 透视观察相机：看 (0, 2, 0) 正前方。
/// 对于尺寸特殊的场景，case 自己再加一个相机即可。
pub fn spawn_default_camera(commands: &mut Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(9.0, 4.5, 14.0).looking_at(Vec3::new(0.0, 2.0, 0.0), Vec3::Y),
        bevy::render::view::Hdr,
    ));
}

/// 60×60 土色地面，不带阴影投射器（避免地面被自己的影子染得漆黑）。
/// 颜色写在顶点色里，所以任何"白底 × 顶点色"的材质都能显示。
pub fn spawn_ground(commands: &mut Commands, meshes: &mut Assets<Mesh>, mat: Handle<StandardMaterial>) {
    let mut g = Cuboid::new(60.0, 0.4, 60.0).mesh().build();
    g = g.translated_by(Vec3::new(0.0, -0.2, 0.0));
    let n = g.count_vertices();
    let soil = Color::srgb_u8(0x6b, 0x6b, 0x5e).to_linear();
    g.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[soil.red, soil.green, soil.blue, 1.0]; n]);
    g.duplicate_vertices();
    g.compute_flat_normals();
    commands.spawn((
        Mesh3d(meshes.add(g)),
        MeshMaterial3d(mat),
        Transform::default(),
        NotShadowCaster,
    ));
}

/// 共享"素面白材质"：地面、碎砖之类不需要贴图的物体用这个。
pub fn plain_material(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        ..default()
    })
}

// ── 通用网格工具（跨 case 可能复用）─────────────────────────────────

/// 合并多个小网格 + 顶点色：
///   - 所有 mesh 必须已经有 Position/Normal/UV/Color 四个属性
///   - 合并成一个 TriangleList 大网格（扁平着色）
/// 用途：chunk 合并、墙体拼装、程序化道具拼装。
pub fn merge_flat(parts: Vec<(Mesh, [f32; 4])>) -> Mesh {
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut norm: Vec<[f32; 3]> = Vec::new();
    let mut uv: Vec<[f32; 2]> = Vec::new();
    let mut col: Vec<[f32; 4]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    for (m, tint) in parts {
        let base = pos.len() as u32;
        let Some(p) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { continue };
        let Some(n) = m.attribute(Mesh::ATTRIBUTE_NORMAL) else { continue };
        let Some(u) = m.attribute(Mesh::ATTRIBUTE_UV_0) else { continue };
        use bevy::mesh::VertexAttributeValues::*;
        if let (Float32x3(p), Float32x3(n), Float32x2(u)) = (p, n, u) {
            let count = p.len();
            pos.extend(p.iter().copied());
            norm.extend(n.iter().copied());
            uv.extend(u.iter().copied());
            col.extend(std::iter::repeat(tint).take(count));
            if let Some(Float32x4(c)) = m.attribute(Mesh::ATTRIBUTE_COLOR) {
                // 如果原 mesh 自己带顶点色，就和 tint 相乘（两个机制互补）
                for i in 0..count {
                    let (a, b) = (c[i], tint);
                    col[base as usize + i] =
                        [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]];
                }
            }
            match m.indices().unwrap() {
                bevy::mesh::Indices::U32(v) => idx.extend(v.iter().map(|&i| i + base)),
                _ => panic!("merge_flat only supports U32 indices"),
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, norm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(bevy::mesh::Indices::U32(idx))
}

/// 给 mesh 每个顶点写同一个颜色。
pub fn tint(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

/// 镜像 UV（u→1-u / v→1-v）：让同一张贴图有 4 种朝向，避免纹理重复感。
pub fn flip_uv(mut m: Mesh, fx: bool, fy: bool) -> Mesh {
    if !fx && !fy {
        return m;
    }
    use bevy::mesh::VertexAttributeValues::Float32x2;
    if let Some(Float32x2(uvs)) = m.attribute(Mesh::ATTRIBUTE_UV_0) {
        let flipped: Vec<[f32; 2]> = uvs
            .iter()
            .map(|[u, v]| [if fx { 1.0 - u } else { *u }, if fy { 1.0 - v } else { *v }])
            .collect();
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, flipped);
    }
    m
}
