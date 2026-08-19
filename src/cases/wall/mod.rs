//! destructible_wall case 的私有模块：墙数据层 + chunk 网格生成。
//!
//! 【第 1 步】WallData：一面墙只是一张 bool 表。打洞 = 改表格。
//! 【第 2 步】build_chunk_mesh：把表格变成 Mesh。
//!   - 只遍历活着的砖（死砖不产生三角形）
//!   - 一面墙切成若干 chunk，每 chunk 一个合并网格 → 打掉砖只重建那一个 chunk
//!
//! 教学要点：数据下标 (c,y,x) 和世界坐标是两回事，
//! 中间只隔两个纯函数：block_center（正向）和 block_from_point（逆向）。

use bevy::asset::RenderAssetUsages;
use bevy::math::primitives::Cuboid;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;

// 复用 common 的 merge/tint/flip_uv，tex 的 brick_texture + hash2
use destr::common::{flip_uv, merge_flat, tint};
use destr::tex::hash2;

// ── 尺寸常量（全部由此推导，禁止魔法数字）────────────────────────────
pub const BLOCK_W: f32 = 1.0; // 砖宽（X）
pub const BLOCK_H: f32 = 0.5; // 砖高（Y，一皮砖）
pub const BLOCK_D: f32 = 0.6; // 砖厚（Z）
pub const COLS: usize = 12; // 墙宽 = 12 块砖
pub const COURSES: usize = 10; // 墙高 = 10 皮
pub const LAYERS: usize = 2; // 墙厚 = 2 皮（打穿要打两层！）

// ── chunk 划分（第 2 步核心：破坏时只重建局部）──────────────────────
pub const CHUNK_COLS: usize = 4; // 每块 chunk 4 列宽
pub const CHUNK_ROWS: usize = 5; // 每块 chunk 5 皮高
pub fn chunks_x() -> usize {
    COLS.div_ceil(CHUNK_COLS)
} // 12/4 = 3
pub fn chunks_y() -> usize {
    COURSES.div_ceil(CHUNK_ROWS)
} // 10/5 = 2
/// 砖 (x,y) 属于哪块 chunk
pub fn chunk_of(x: usize, y: usize) -> (usize, usize) {
    (x / CHUNK_COLS, y / CHUNK_ROWS)
}

// ── 调色板（低饱和三档石色 + 确定性哈希选色，防"玩具感"）─────────────
pub const STONE_LIGHT: u32 = 0xb9b5a8;
pub const STONE_MID: u32 = 0x9a968a;
pub const STONE_DARK: u32 = 0x6f6b62;

fn srgb(hex: u32) -> Color {
    Color::srgb_u8(((hex >> 16) & 0xff) as u8, ((hex >> 8) & 0xff) as u8, (hex & 0xff) as u8)
}
fn lin(hex: u32) -> [f32; 4] {
    let l = srgb(hex).to_linear();
    [l.red, l.green, l.blue, 1.0]
}
/// 每块砖在三档石色里"随机"挑一档 —— 颜色统一是塑料感的第一来源
fn block_color(c: usize, y: usize, x: usize) -> [f32; 4] {
    let r = hash2(x as i32 * 31 + c as i32, y as i32 * 7);
    if r < 0.18 {
        lin(STONE_DARK)
    } else if r < 0.55 {
        lin(STONE_MID)
    } else {
        lin(STONE_LIGHT)
    }
}

// ── 第 1 步：数据层 ────────────────────────────────────────────────
#[derive(Resource)]
pub struct WallData {
    blocks: Vec<Vec<Vec<bool>>>, // [c][y][x]
}

impl WallData {
    pub fn new() -> Self {
        WallData { blocks: vec![vec![vec![true; COLS]; COURSES]; LAYERS] }
    }

    pub fn alive(&self, c: usize, y: usize, x: usize) -> bool {
        *self
            .blocks
            .get(c)
            .and_then(|layer| layer.get(y))
            .and_then(|row| row.get(x))
            .unwrap_or(&false)
    }

    pub fn destroy(&mut self, c: usize, y: usize, x: usize) {
        if self.alive(c, y, x) {
            self.blocks[c][y][x] = false;
        }
    }

    pub fn alive_count(&self) -> usize {
        self.blocks
            .iter()
            .flat_map(|l| l.iter())
            .flat_map(|r| r.iter())
            .filter(|&&b| b)
            .count()
    }
}

// ── 网格坐标 ↔ 世界坐标（第 3 步点击拾取的关键）─────────────────────

/// 数据下标 → 砖块中心的世界坐标。
/// 注意错缝（running bond）：奇数层整体偏移半砖 —— 这是"砖墙像砖墙"的灵魂。
pub fn block_center(c: usize, y: usize, x: usize) -> Vec3 {
    let wall_w = COLS as f32 * BLOCK_W;
    let wall_t = LAYERS as f32 * BLOCK_D;
    let bond = if y % 2 == 1 { BLOCK_W * 0.5 } else { 0.0 };
    Vec3::new(
        -wall_w / 2.0 + BLOCK_W / 2.0 + x as f32 * BLOCK_W + bond,
        BLOCK_H / 2.0 + y as f32 * BLOCK_H,
        -wall_t / 2.0 + BLOCK_D / 2.0 + c as f32 * BLOCK_D,
    )
}

/// 世界坐标 → 数据下标（block_center 的逆运算）。
/// 输入应是砖块【内部】的点（见调用处：命中点沿法线后退 1cm 再进来）。
pub fn block_from_point(p: Vec3) -> Option<(usize, usize, usize)> {
    let wall_w = COLS as f32 * BLOCK_W;
    let wall_t = LAYERS as f32 * BLOCK_D;
    if p.x < -wall_w / 2.0 || p.x > wall_w / 2.0 + BLOCK_W * 0.5 {
        return None;
    }
    if p.z < -wall_t / 2.0 || p.z > wall_t / 2.0 {
        return None;
    }
    let y = (p.y / BLOCK_H).floor() as usize;
    if y >= COURSES {
        return None;
    }
    // 先扣掉该层的错缝偏移，再除以砖宽
    let bond = if y % 2 == 1 { BLOCK_W * 0.5 } else { 0.0 };
    let mut x = ((p.x + wall_w / 2.0 - bond) / BLOCK_W).floor() as i32;
    x = x.clamp(0, COLS as i32 - 1); // 奇数层最右一块伸出半砖，钳回来
    let c = (((p.z + wall_t / 2.0) / BLOCK_D).floor() as usize).min(LAYERS - 1);
    Some((c, y, x as usize))
}

// ── 第 2 步：数据 → 网格 ──────────────────────────────────────────

/// 生成一块 chunk 的合并网格：遍历该 chunk 范围内的砖，
/// 活着的 → cuboid + 顶点色；死掉的 → 跳过（这就是"破坏"在渲染层的全部含义）。
pub fn build_chunk_mesh(wall: &WallData, cx: usize, cy: usize) -> Mesh {
    let mut parts: Vec<(Mesh, [f32; 4])> = Vec::new();
    let x0 = cx * CHUNK_COLS;
    let y0 = cy * CHUNK_ROWS;
    for c in 0..LAYERS {
        for y in y0..(y0 + CHUNK_ROWS).min(COURSES) {
            for x in x0..(x0 + CHUNK_COLS).min(COLS) {
                if !wall.alive(c, y, x) {
                    continue;
                }
                // 满尺寸砖：砖缝和边框全部画在 tex::brick_texture 里
                let mut m = Cuboid::new(BLOCK_W, BLOCK_H, BLOCK_D).mesh().build();
                m = m.translated_by(block_center(c, y, x));
                // 哈希镜像 UV：同一张贴图 4 种朝向（白送的变化量）
                let fx = hash2(x as i32 * 3 + c as i32, y as i32) < 0.5;
                let fy = hash2(x as i32, y as i32 * 5 + c as i32) < 0.5;
                let m = flip_uv(m, fx, fy);
                parts.push((m, block_color(c, y, x)));
            }
        }
    }
    if parts.is_empty() {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
        return m;
    }
    // 先挨个上顶点色，再合并（复用 common::merge_flat → 做乘算）
    let colored: Vec<_> = parts.into_iter().map(|(m, c)| (tint(m, c), [1.0, 1.0, 1.0, 1.0])).collect();
    let mut base = merge_flat(colored);
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}

// ── 验证出口：ASCII 截面（内存截面扫描）────────────────────────────
pub fn print_wall_section(wall: &WallData, c: usize) {
    println!("── 墙截面（厚度层 c={}）──", c);
    for y in (0..COURSES).rev() {
        let mut row = String::new();
        for x in 0..COLS {
            row.push(if wall.alive(c, y, x) { '▓' } else { '·' });
        }
        println!("层{:2}  {}", y, row);
    }
    println!("剩余砖块: {}/{}", wall.alive_count(), LAYERS * COLS * COURSES);
}
