//! 场景基础元素库：面向"搭大场景时逐个元素查参数"的需求。
//!
//! 哲学：每个基础元素（砖、水泥块、碎砖、门槛…）都实现同一个 [`Element`] trait，
//! 你只需要把具体类型或实例丢进来，尺寸 / 名字 / 调色板 / 默认 Mesh / 默认材质 /
//! 默认贴图全部从 Element API 走，**不再在 case 代码里手写 BLOCK_W / BLOCK_H 常量**。
//!
//! # 三种"取尺寸"的写法，按你喜欢选
//!
//! ```ignore
//! use destr::elements::{Brick, CementBlock, Element};
//!
//! // 1) 类型级常量（零开销，推荐——编译器已知）
//! assert_eq!(Brick::SIZE, Vec3::new(Brick::WIDTH, Brick::HEIGHT, Brick::DEPTH));
//!
//! // 2) 实例方法（你已经有一个 `element: T` 的情况）
//! let brick = Brick;
//! assert_eq!(brick.get_length().x, Brick::WIDTH);   // X 向 = Length
//! assert_eq!(brick.get_length().y, Brick::HEIGHT);  // Y 向 = Height
//! assert_eq!(brick.get_length().z, Brick::DEPTH);   // Z 向 = Depth
//!
//! // 3) 按轴单独拿：最贴近用户说的"getLength(砖)"
//! assert_eq!(brick.get_length_x(), Brick::WIDTH);
//! assert_eq!(brick.get_length_y(), Brick::HEIGHT);
//! assert_eq!(brick.get_length_z(), Brick::DEPTH);
//! ```
//!
//! # 三种当前内置元素
//!
//! | 类型 | 中文名 | 典型用途 |
//! |---|---|---|
//! | [`Brick`] | 红/灰砖 | 砌体墙、道路铺砖、装饰小柱 |
//! | [`CementBlock`] | 水泥块 | 混凝土墙、厂房立柱、路缘石 |
//! | [`DebrisPiece`] | 碎砖/碎块 | 破坏后掉落物、废墟地面装饰、静态碎块堆 |

use bevy::asset::RenderAssetUsages;
use bevy::math::primitives::Cuboid;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::common::{flip_uv, tint};
use crate::tex::{bark_texture, brick_texture, cement_texture, debris_texture, hash2, rock_texture};

// ── 统一尺寸类型：X=宽/Length，Y=高/Height，Z=厚/Depth ─────────────
//
// 行业里"Length(长度) / Width(宽度)"常混着用，这里同时提供两套名字：
//   get_length_xyz  →  x=长  y=高  z=厚（建筑术语习惯）
//   size()          →  (X, Y, Z) 就是 Vec3（引擎/图形术语习惯）
// 底层都走 SIZE / WIDTH / HEIGHT / DEPTH 常量，结果完全一致。

/// 一个场景基础元素。
///
/// 实现 trait 的类型都是"零大小标记类型"（ZST）：`Brick`、`CementBlock`、`DebrisPiece`。
/// 你可以 `let x = Brick;` 拿实例调用方法，也可以直接用关联常量。
/// 两种方式 **零开销**。
pub trait Element: Default + Copy + Clone + Send + Sync + 'static {
    // ---- 尺寸（类型级常量，每个元素自己写一套默认值） ----
    const WIDTH:  f32; /// = X，也对应 Length / 建筑上说的"长"
    const HEIGHT: f32; /// = Y
    const DEPTH:  f32; /// = Z，也对应 Thickness / "厚度"
    const NAME: &'static str;

    /// 整个元素的包围盒尺寸（WIDTH × HEIGHT × DEPTH），等于 `Vec3::new(W,H,D)`。
    const SIZE: Vec3 = Vec3::new(Self::WIDTH, Self::HEIGHT, Self::DEPTH);

    /// 三档调色板（亮/中/暗），16 进制 sRGB，带透明通道无效。
    const PALETTE: [u32; 3];

    // ---- 派生尺寸：实例方法，`element.get_length().x` 风格 ----

    /// 全尺寸向量（等价于 `Self::SIZE`）。
    #[inline(always)]
    fn get_size(&self) -> Vec3 { Self::SIZE }
    /// 三个轴的"长度"（= get_size 的各分量拆开）。
    #[inline(always)]
    fn get_length(&self)  -> Vec3 { Self::SIZE }
    #[inline(always)]
    fn get_length_x(&self) -> f32 { Self::WIDTH }
    #[inline(always)]
    fn get_length_y(&self) -> f32 { Self::HEIGHT }
    #[inline(always)]
    fn get_length_z(&self) -> f32 { Self::DEPTH }
    /// 别名：图形习惯说"宽/高/深"。
    #[inline(always)]
    fn get_width(&self)  -> f32 { Self::WIDTH }
    #[inline(always)]
    fn get_height(&self) -> f32 { Self::HEIGHT }
    #[inline(always)]
    fn get_depth(&self)  -> f32 { Self::DEPTH }

    // ---- 人类可读名 ----
    fn name(&self) -> &'static str { Self::NAME }

    // ---- Mesh / 材质 / 贴图的默认工厂 ----

    /// 返回"默认大小的一块元素"的 Mesh（居中于原点，不做平移）。
    /// 默认用一个 Cuboid；想换成自定义网格请覆盖。
    fn base_mesh() -> Mesh {
        Cuboid::new(Self::WIDTH, Self::HEIGHT, Self::DEPTH).mesh().build()
    }

    /// 纹理生成函数：返回该元素的贴图。
    /// 默认走 [`brick_texture`]，水泥块等覆盖成自己的。
    fn default_image() -> Image;

    /// 为一块"具体坐标下"的元素生成 **带选色顶点色 + UV 镜像** 的 mesh。
    ///
    /// 这是做墙/堆块大场景时最常用的：同一张贴图 + 三档哈希选色 + UV 翻转，
    /// 肉眼上"每块都不同"，内存只涨了顶点色（16 字节/顶点，极便宜）。
    ///
    /// `(cx, cy, cz, s)` 是选种子用的坐标/随机量；`translate` 是是否把 mesh 平移到世界坐标。
    fn painted_mesh(
        cx: i32, cy: i32, cz: i32, s: i32,
        world_center: Vec3, translate_mesh: bool,
    ) -> Mesh {
        let mut m = Self::base_mesh();
        if translate_mesh {
            m = m.translated_by(world_center);
        }
        // UV 翻转 2×2 = 4 种朝向，白送的变化
        let fx = hash2(cx * 3 + cz, cy) < 0.5;
        let fy = hash2(cy * 5 + s, cx) < 0.5;
        let m = flip_uv(m, fx, fy);
        // 三档选色
        let col = sample_palette(Self::PALETTE, cx, cy, cz, s);
        tint(m, col)
    }

    /// 直接拿到 `Assets<StandardMaterial>` 里注册好的材质句柄。
    ///
    /// 用法：
    /// ```ignore
    /// let mat = Brick::default_material(&mut materials, &mut images, Color::WHITE, 0.95);
    /// ```
    fn default_material(
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        base_tint: Color,
        roughness: f32,
    ) -> Handle<StandardMaterial> {
        let tex = images.add(Self::default_image());
        materials.add(StandardMaterial {
            base_color: base_tint,
            base_color_texture: Some(tex),
            perceptual_roughness: roughness,
            ..default()
        })
    }
}

// ── 调色板工具：三档色 + 哈希选色 ───────────────────────────────────

fn srgb(hex: u32) -> Color {
    Color::srgb_u8(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}
fn lin4(hex: u32) -> [f32; 4] {
    let l = srgb(hex).to_linear();
    [l.red, l.green, l.blue, 1.0]
}
fn sample_palette(pal: [u32; 3], cx: i32, cy: i32, cz: i32, s: i32) -> [f32; 4] {
    let r = hash2(cx.wrapping_mul(31).wrapping_add(cz.wrapping_mul(7)), cy.wrapping_mul(13).wrapping_add(s));
    if r < 0.18 { lin4(pal[2]) }
    else if r < 0.55 { lin4(pal[1]) }
    else { lin4(pal[0]) }
}

// ─────────────────────────────────────────────────────────────────────
// Element 1: 砖
// ─────────────────────────────────────────────────────────────────────

/// 标准建筑砖：1.0 × 0.5 × 0.6（W × H × D）。
///
/// 错缝砌墙用这个宽度 1.0、高度 0.5 正好是 2:1，running bond 最自然。
#[derive(Debug, Default, Copy, Clone)]
pub struct Brick;

impl Element for Brick {
    const WIDTH:  f32 = 1.0;
    const HEIGHT: f32 = 0.5;
    const DEPTH:  f32 = 0.6;
    const NAME: &'static str = "Brick";
    // 三档灰石色（和原墙 case 保持一致，重构零变化）
    const PALETTE: [u32; 3] = [0xb9b5a8, 0x9a968a, 0x6f6b62];

    fn default_image() -> Image { brick_texture() }
}

// ─────────────────────────────────────────────────────────────────────
// Element 2: 水泥块
// ─────────────────────────────────────────────────────────────────────

/// 标准混凝土块（加气块 / cement block）：1.0 × 0.6 × 0.6（W × H × D）。
///
/// 混凝土块比砖更大、色调冷、接缝明显。
#[derive(Debug, Default, Copy, Clone)]
pub struct CementBlock;

impl Element for CementBlock {
    const WIDTH:  f32 = 1.0;
    const HEIGHT: f32 = 0.6;
    const DEPTH:  f32 = 0.6;
    const NAME: &'static str = "CementBlock";
    // 冷灰三档（水泥本色偏蓝灰）
    const PALETTE: [u32; 3] = [0xb9bfc4, 0x8d949b, 0x5e656c];

    fn default_image() -> Image { cement_texture() }
}

// ─────────────────────────────────────────────────────────────────────
// Element 3: 碎砖
// ─────────────────────────────────────────────────────────────────────

/// 破坏后掉地上的碎砖块：0.22 × 0.22 × 0.22 的小立方。
///
/// 作为废墟装饰、静态碎块堆，也是 destructible_wall 里 Debris 的统一基准尺寸。
/// （实际掉落物一般 `scale = 0.6..1.4` 随机拉一下，避免玩具感）
#[derive(Debug, Default, Copy, Clone)]
pub struct DebrisPiece;

impl Element for DebrisPiece {
    const WIDTH:  f32 = 0.22;
    const HEIGHT: f32 = 0.22;
    const DEPTH:  f32 = 0.22;
    const NAME: &'static str = "DebrisPiece";
    // 偏脏的中灰色（碎片颜色偏一致，不然太跳）
    const PALETTE: [u32; 3] = [0xa5a195, 0x85817a, 0x66625c];

    // ✅ 用户要求"碎砖不该有边框"：不再复用 brick_texture（带灰缝），改用 debris_texture（纯噪点）
    fn default_image() -> Image { debris_texture() }
}

// ── 自由函数：给定 (尺寸 × 缩放) 算 half-extent ────────────────────
//
// 很多系统（比如 destructible_wall 里的 Debris 地面回弹）需要知道
// 当下缩放后的 half-extent。提供一个统一入口，case 作者不用再写死 0.22/2。

/// 给定"元素的 size 向量"和"Transform scale"，返回 AABB half-extent。
pub fn half_after_scale(size: Vec3, scale: Vec3) -> Vec3 {
    Vec3::new(size.x * scale.x, size.y * scale.y, size.z * scale.z) * 0.5
}

// ── 帮助：造一个"空的三角 List Mesh"（chunk 全空时用）───────────────

/// 一个空的 TriangleList mesh（chunk 里什么也没有时返回它，不 panic）。
pub fn empty_triangle_mesh() -> Mesh {
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
    m.insert_indices(Indices::U32(vec![]));
    m
}

// ═════════════════════════════════════════════════════════════════════════
//  工具：手搓 3 种非立方 Mesh（不规则石块 / 拱曲面砖 / 歪曲圆柱树干）
// ═════════════════════════════════════════════════════════════════════════

// 策略：每个工具返回"ATTRIBUTE_POSITION / NORMAL / UV / INDICES 四件套齐全"的 Mesh，
// 这样即便 base_mesh 不走 Cuboid，default_material/painted_mesh 也能直接用（因为
// painted_mesh 只会做"UV flip / translate / tint"，不依赖输入网格的拓扑）。

// ---------- 工具 1：不规则石块（立方体 8 顶点抖动 + 6 面 2 tris / 面）-----------

/// 构造一个边长 1 的"抖动立方体"多面体 mesh。
///
/// - 8 个顶点每个都加一个 seed 决定的随机位移（displace ~18% 的 half-extent，
///   保持对称中心仍在原点，所以 SIZE 作为 AABB 仍然靠谱）。
/// - 6 个面每个 2 个三角，共 36 个顶点位置（每个面独立顶点 = 平面法线硬边，石头切面更锐利）。
pub fn irregular_rock_mesh(size: Vec3, seed: i32) -> Mesh {
    let h = size * 0.5;
    // 8 个角点：按 corner_idx → hash 算出位移向量
    let mut corners: [Vec3; 8] = [Vec3::ZERO; 8];
    for i in 0..8 {
        let cx = ((i & 1) as f32) * 2.0 - 1.0;   // ±1
        let cy = (((i >> 1) & 1) as f32) * 2.0 - 1.0;
        let cz = (((i >> 2) & 1) as f32) * 2.0 - 1.0;
        let base = Vec3::new(h.x * cx, h.y * cy, h.z * cz);
        let dx = (hash2(i as i32 + seed * 101, 123) - 0.5) * h.x * 0.36;
        let dy = (hash2(i as i32, 7 + seed * 53) - 0.5) * h.y * 0.36;
        let dz = (hash2(i as i32, seed * 17 - 31) - 0.5) * h.z * 0.36;
        corners[i] = base + Vec3::new(dx, dy, dz);
    }
    // 6 个面：每个面按"从面外看逆时针"的顺序 4 顶点
    //   索引顺序：corner_index = |bit0 x| bit1 y| bit2 z|  （和上面写的相同）
    //   右手法则：法线朝外
    let faces: [[usize; 4]; 6] = [
        // +X (x=1 面): (1,0,0)->(1,1,0)->(1,1,1)->(1,0,1)   右手法则 → +X
        [1, 3, 7, 5],
        // -X
        [0, 4, 6, 2],
        // +Y
        [2, 6, 7, 3],
        // -Y
        [0, 1, 5, 4],
        // +Z
        [4, 5, 7, 6],
        // -Z
        [0, 2, 3, 1],
    ];
    let face_normals: [Vec3; 6] = [
        Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z,
    ];

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(6 * 4);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(6 * 4);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(6 * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(6 * 6);

    for (f, face) in faces.iter().enumerate() {
        let base = positions.len() as u32;
        // 4 个 UV 角
        let quad_uv: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        for (k, &c) in face.iter().enumerate() {
            positions.push(corners[c].into());
            normals.push(face_normals[f].into());
            uvs.push(quad_uv[k]);
        }
        indices.push(base); indices.push(base + 1); indices.push(base + 2);
        indices.push(base); indices.push(base + 2); indices.push(base + 3);
    }

    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL,   normals);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0,     uvs);
    m.insert_indices(Indices::U32(indices));
    m
}

// ---------- 工具 2：拱曲面楔形砖（拼圆拱/圆柱曲面的基本单元）-----------

/// 一个弧度为 `arc_rad`、厚度 `thick`、轴向高度 `height` 的弧形砖（楔形截面）。
///
/// 空间约定：
///   - 曲面在 **XZ 平面**弯曲（所以一整条拱向上就是 Y 为高度，XZ 平面做拱曲线）。
///   - 外半径 `r_outer`，内半径 `r_outer - thick`。
///   - 径向范围：`[-arc_rad/2, arc_rad/2]`（绕 +Y 轴），元素中心在原点处。
///   - 高度（Y 方向，沿拱的柱轴）=`height`，中心 ±height/2。
///
/// 用它拼拱：N 块并列，每块 `arc_rad = π/N`，首尾接起来就是半圆拱。
/// 用它拼圆柱：每块 `arc_rad = 2π/N`，绕一圈接起来。
pub fn arch_brick_mesh(r_outer: f32, thick: f32, arc_rad: f32, height: f32, slices: u32) -> Mesh {
    let r_inner = (r_outer - thick).max(0.001);
    let h = height * 0.5;
    let slices = slices.max(2);

    // 一个"横截面（XZ 平面）"有 2 × (slices+1) 顶点：
    //   第 0 行 = inner 半径上圆弧顶点（i from 0..=slices）
    //   第 1 行 = outer 半径上圆弧顶点
    // 然后每个 slice 有 4 个侧（前/后/内/外）+ 两个端面，共 6 组面。
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    // 预先算每段的角度坐标
    let theta = |i: u32| -> f32 {
        let t = i as f32 / slices as f32;   // 0..=1
        -arc_rad * 0.5 + arc_rad * t
    };

    // ── 侧面 1：外弧圆柱面（最重要的"曲面"）───────────────
    let outer_start = positions.len() as u32;
    for i in 0..=slices {
        let a = theta(i);
        let (s, c) = a.sin_cos();
        // X = sin*r, Z = cos*r（保证正前方 +Z 是 θ=0 方向）
        let px = r_outer * s;
        let pz = r_outer * c;
        // 下点、上点
        positions.push([px, -h, pz]);
        positions.push([px,  h, pz]);
        // 法线（径向向外）
        let nx = s;
        let nz = c;
        normals.push([nx, 0.0, nz]);
        normals.push([nx, 0.0, nz]);
        // UV：u = 0..1 (θ 方向)，v = 0..1 (高度)
        let u = i as f32 / slices as f32;
        uvs.push([u, 0.0]);
        uvs.push([u, 1.0]);
    }
    for i in 0..slices {
        let base = outer_start + (i * 2) as u32;
        indices.push(base); indices.push(base + 1); indices.push(base + 3);
        indices.push(base); indices.push(base + 3); indices.push(base + 2);
    }

    // ── 侧面 2：内弧圆柱面（法线朝内）───────────────
    let inner_start = positions.len() as u32;
    for i in 0..=slices {
        let a = theta(i);
        let (s, c) = a.sin_cos();
        let px = r_inner * s;
        let pz = r_inner * c;
        positions.push([px, -h, pz]);
        positions.push([px,  h, pz]);
        // 法线：朝内（-径向）
        normals.push([-s, 0.0, -c]);
        normals.push([-s, 0.0, -c]);
        let u = i as f32 / slices as f32;
        uvs.push([u, 0.0]);
        uvs.push([u, 1.0]);
    }
    for i in 0..slices {
        // 注意 winding 反向
        let base = inner_start + (i * 2) as u32;
        indices.push(base); indices.push(base + 2); indices.push(base + 3);
        indices.push(base); indices.push(base + 3); indices.push(base + 1);
    }

    // ── 顶部面（Y=+h）───────────────
    let top_start = positions.len() as u32;
    for i in 0..=slices {
        let a = theta(i);
        let (s, c) = a.sin_cos();
        positions.push([r_inner * s, h, r_inner * c]);
        positions.push([r_outer * s, h, r_outer * c]);
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        let u = i as f32 / slices as f32;
        uvs.push([u, 0.0]);
        uvs.push([u, 1.0]);
    }
    for i in 0..slices {
        let base = top_start + (i * 2) as u32;
        indices.push(base); indices.push(base + 1); indices.push(base + 3);
        indices.push(base); indices.push(base + 3); indices.push(base + 2);
    }

    // ── 底部面（Y=-h）───────────────
    let bot_start = positions.len() as u32;
    for i in 0..=slices {
        let a = theta(i);
        let (s, c) = a.sin_cos();
        positions.push([r_inner * s, -h, r_inner * c]);
        positions.push([r_outer * s, -h, r_outer * c]);
        normals.push([0.0, -1.0, 0.0]);
        normals.push([0.0, -1.0, 0.0]);
        let u = i as f32 / slices as f32;
        uvs.push([u, 0.0]);
        uvs.push([u, 1.0]);
    }
    for i in 0..slices {
        let base = bot_start + (i * 2) as u32;
        indices.push(base); indices.push(base + 2); indices.push(base + 3);
        indices.push(base); indices.push(base + 3); indices.push(base + 1);
    }

    // ── 端面（θ = -arc/2 和 +arc/2，两个梯形）───────────────
    // 端面 1：θ = -arc/2
    let end1_start = positions.len() as u32;
    let a0 = theta(0);
    let (s0, c0) = a0.sin_cos();
    // 从端面"外"看的逆时针顺序（法线朝 -π/2 方向）
    let n1 = Vec3::new(-c0, 0.0, s0); // 法向：θ 减小方向
    for (r, v) in [(r_inner, -h), (r_outer, -h), (r_outer, h), (r_inner, h)] {
        positions.push([r * s0, v, r * c0]);
        normals.push(n1.into());
    }
    uvs.push([0.0, 0.0]); uvs.push([1.0, 0.0]); uvs.push([1.0, 1.0]); uvs.push([0.0, 1.0]);
    indices.push(end1_start); indices.push(end1_start + 1); indices.push(end1_start + 2);
    indices.push(end1_start); indices.push(end1_start + 2); indices.push(end1_start + 3);

    // 端面 2：θ = +arc/2
    let end2_start = positions.len() as u32;
    let a1 = theta(slices);
    let (s1, c1) = a1.sin_cos();
    let n2 = Vec3::new(c1, 0.0, -s1); // 法向：θ 增大方向
    for (r, v) in [(r_inner, -h), (r_inner, h), (r_outer, h), (r_outer, -h)] {
        positions.push([r * s1, v, r * c1]);
        normals.push(n2.into());
    }
    uvs.push([0.0, 0.0]); uvs.push([0.0, 1.0]); uvs.push([1.0, 1.0]); uvs.push([1.0, 0.0]);
    indices.push(end2_start); indices.push(end2_start + 1); indices.push(end2_start + 2);
    indices.push(end2_start); indices.push(end2_start + 2); indices.push(end2_start + 3);

    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL,   normals);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0,     uvs);
    m.insert_indices(Indices::U32(indices));
    m
}

// ---------- 工具 3：歪曲圆柱（树干）-----------

/// 构造一个"沿 Y 轴向上的分段圆柱，每圈半径不同 + 顶点径向抖动 + 整体弯向 (X,Z) 曲线"。
///
/// 参数：
///   - height：整体高度（Y 向）
///   - base_radius：根部半径
///   - tip_radius：  顶部半径（一般 < base_radius，模拟锥度）
///   - radial_slices：圆周方向分片数（18 起步比较圆）
///   - vertical_slices：高度方向分段数（6 起步）
///   - bend：整体弯曲方向的"最大偏移向量"（XZ 平面，只影响 X/Z 位置，不影响高度）
///   - seed：顶点扰动种子（0..seed 变化时每棵树形状不同）
pub fn curved_trunk_mesh(
    height: f32,
    base_radius: f32,
    tip_radius: f32,
    radial_slices: u32,
    vertical_slices: u32,
    bend: Vec2,
    seed: i32,
) -> Mesh {
    let rs = radial_slices.max(4);
    let vs = vertical_slices.max(2);

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(((rs + 1) * (vs + 1)) as usize);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(positions.capacity());
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(positions.capacity());
    let mut indices: Vec<u32> = Vec::with_capacity((rs * vs * 6) as usize);

    for j in 0..=vs {
        let t = j as f32 / vs as f32;           // 0..=1（底→顶）
        // 弯曲抛物线：t*2(2-t) = 2t - t^2 让根部不弯曲、顶部弯到最厉害
        let curve = t * (2.0 - t);
        let offset_x = bend.x * curve;
        let offset_z = bend.y * curve;
        let radius = base_radius.lerp(tip_radius, t);
        for i in 0..=rs {
            let a = (i as f32 / rs as f32) * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            // 顶点扰动：再给半径加一点 ±10% 高频抖动 + 一点低频
            let k1 = hash2(i as i32, j as i32 + seed);
            let k2 = hash2(i as i32 / 3, (j as i32).wrapping_mul(17).wrapping_add(seed));
            let jitter = (k1 - 0.5) * 0.18 + (k2 - 0.5) * 0.10;
            let r = radius * (1.0 + jitter);
            let px = c * r + offset_x;
            let pz = s * r + offset_z;
            let py = t * height; // 0..height（原点在底部中心；方便放地上时 translate_y 不动）
            positions.push([px, py, pz]);
            // 近似法线（忽略弯曲对法线的影响，够用）
            normals.push([c, 0.0, s]);
            // UV：u = 绕一圈 (0..1)，v = 高度 (0..1)
            uvs.push([i as f32 / rs as f32, t]);
        }
    }

    // 四边形网格 → 两三角
    for j in 0..vs {
        for i in 0..rs {
            let a = (j * (rs + 1) + i) as u32;
            let b = a + 1;
            let cc = a + (rs + 1) as u32;
            let d = cc + 1;
            indices.push(a); indices.push(cc); indices.push(d);
            indices.push(a); indices.push(d); indices.push(b);
        }
    }

    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL,   normals);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0,     uvs);
    m.insert_indices(Indices::U32(indices));
    m
}

// ═════════════════════════════════════════════════════════════════════════
//  Element 4: IrregularRock（不规则石块）
// ═════════════════════════════════════════════════════════════════════════

/// 天然不规则石块：抖动顶点的多面体，默认 0.9 × 0.75 × 0.9（稍扁的河卵石感）。
///
/// base_mesh 固定 seed=0 保证"陈列时每次长一样"。
/// 要在场景里撒一堆不同形状，用 `<IrregularRock as Element>::rock_mesh(seed)` 自己传 seed。
#[derive(Debug, Default, Copy, Clone)]
pub struct IrregularRock;

impl IrregularRock {
    /// 按种子生成一块"同尺寸、不同形状"的石块 mesh。
    pub fn rock_mesh(seed: i32) -> Mesh { irregular_rock_mesh(Self::SIZE, seed) }
}

impl Element for IrregularRock {
    const WIDTH:  f32 = 0.90;
    const HEIGHT: f32 = 0.75;
    const DEPTH:  f32 = 0.90;
    const NAME: &'static str = "IrregularRock";
    // 冷灰岩石三档
    const PALETTE: [u32; 3] = [0x9da6ae, 0x6f7880, 0x4a5258];

    fn base_mesh() -> Mesh { irregular_rock_mesh(Self::SIZE, 0) }
    fn default_image() -> Image { rock_texture() }
}

// ═════════════════════════════════════════════════════════════════════════
//  Element 5: ArchBrick（拱曲面楔形砖）
// ═════════════════════════════════════════════════════════════════════════

/// 拱/圆柱曲面用的楔形砖。
///
/// 默认配置（一块放陈列能看到弧度即可）：
///   - 外半径 2.0，厚 0.4，弧 30° = π/6，高 0.5，径向 8 分片。
///   - AABB SIZE：约 (2.0·2·sin(π/12) = 1.035) × 0.5 × 0.4。
///
/// 想搭半圆拱？拿 12 块每块 15°（π/12），每块绕原点旋转 +k·15° 即可。
#[derive(Debug, Default, Copy, Clone)]
pub struct ArchBrick;

impl ArchBrick {
    pub const R_OUTER:  f32 = 2.0;
    pub const THICK:    f32 = 0.4;
    pub const ARC_RAD:  f32 = std::f32::consts::PI / 6.0; // 30°
    pub const SLICES:   u32 = 8;
}

impl Element for ArchBrick {
    // AABB 估算：2·R·sin(arc/2) ≈ 2·2·sin(15°) ≈ 2·2·0.2588 ≈ 1.0353
    // 这里 sin() 不是 const fn，直接写死提前算好的常数值（保留 6 位小数）。
    const WIDTH:  f32 = 1.035_276;
    const HEIGHT: f32 = 0.5;
    const DEPTH:  f32 = ArchBrick::THICK;
    const NAME: &'static str = "ArchBrick";
    // 带暖调的橙黄砂砖（拱门常见）
    const PALETTE: [u32; 3] = [0xd9b47a, 0xb99060, 0x8a6a44];

    fn base_mesh() -> Mesh {
        arch_brick_mesh(Self::R_OUTER, Self::THICK, Self::ARC_RAD, Self::HEIGHT, Self::SLICES)
    }
    fn default_image() -> Image { brick_texture() } // 拱曲面也用砖面
}

// ═════════════════════════════════════════════════════════════════════════
//  Element 6: CurvedCylinderTrunk（歪曲圆柱 / 树干）
// ═════════════════════════════════════════════════════════════════════════

/// 一棵"歪"树的树干：带锥度 + 顶点扰动 + 整体向一个方向弯。
///
/// 默认配置：
///   - 高 3.0，底部半径 0.3，顶部 0.12，
///   - 径向 18 片，轴向 7 段，
///   - 弯曲方向 (0.25, 0.15)（向右前方略微偏）。
///
/// SIZE 给出 AABB 估：(底径 + 2·bend.x) × 高 × (底径 + 2·bend.y)
#[derive(Debug, Default, Copy, Clone)]
pub struct CurvedCylinderTrunk;

impl CurvedCylinderTrunk {
    pub const HEIGHT:        f32  = 3.0;
    pub const BASE_RADIUS:   f32  = 0.30;
    pub const TIP_RADIUS:    f32  = 0.12;
    pub const RADIAL_SLICES: u32  = 18;
    pub const VERT_SLICES:   u32  = 7;
    pub const BEND:          Vec2 = Vec2::new(0.25, 0.15);
    pub const SEED:          i32  = 0;

    /// 按自定义种子+弯曲量再生成一棵（做小树林不重复）。
    pub fn trunk_mesh(seed: i32, bend: Vec2) -> Mesh {
        curved_trunk_mesh(
            Self::HEIGHT, Self::BASE_RADIUS, Self::TIP_RADIUS,
            Self::RADIAL_SLICES, Self::VERT_SLICES,
            bend, seed,
        )
    }
}

impl Element for CurvedCylinderTrunk {
    // 尺寸估：让 AABB 包住底径 + 最大弯曲偏移
    const WIDTH:  f32 = CurvedCylinderTrunk::BASE_RADIUS * 2.0 + CurvedCylinderTrunk::BEND.x.abs() * 2.0;
    const HEIGHT: f32 = CurvedCylinderTrunk::HEIGHT;
    const DEPTH:  f32 = CurvedCylinderTrunk::BASE_RADIUS * 2.0 + CurvedCylinderTrunk::BEND.y.abs() * 2.0;
    const NAME: &'static str = "CurvedCylinderTrunk";
    // 树皮棕褐色三档
    const PALETTE: [u32; 3] = [0x7a5c3f, 0x5c4430, 0x3f2f22];

    fn base_mesh() -> Mesh {
        curved_trunk_mesh(
            Self::HEIGHT, Self::BASE_RADIUS, Self::TIP_RADIUS,
            Self::RADIAL_SLICES, Self::VERT_SLICES,
            Self::BEND, Self::SEED,
        )
    }
    fn default_image() -> Image { bark_texture() }
}
