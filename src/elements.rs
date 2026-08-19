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
use crate::tex::{brick_texture, cement_texture, hash2};

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

    fn default_image() -> Image { brick_texture() } // 碎砖纹理复用砖面，缩放时已经够脏
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
