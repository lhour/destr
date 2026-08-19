//! destructible_wall case 的私有模块：墙数据层 + chunk 网格生成。
//!
//! 【第 1 步】WallData：一面墙只是一张 bool 表。打洞 = 改表格。
//! 【第 2 步】build_chunk_mesh：把表格变成 Mesh（走 [`destr::elements::Brick`]）。
//!   - 只遍历活着的砖（死砖不产生三角形）
//!   - 一面墙切成若干 chunk，每 chunk 一个合并网格 → 打掉砖只重建那一个 chunk
//!
//! 教学要点：数据下标 (c,y,x) 和世界坐标是两回事，
//! 中间只隔两个纯函数：block_center（正向）和 block_from_point（逆向）。

use bevy::prelude::*;

use destr::common::merge_flat;
use destr::elements::{empty_triangle_mesh, Brick, Element};

// ── 墙尺寸：12 × 10 × 2（宽列数 × 皮数 × 厚皮数）──────────────────
//
// 单砖尺寸不再是常量魔法数字！请走：
//   Brick::WIDTH / Brick::HEIGHT / Brick::DEPTH
// 或者实例方法：Brick.get_length_x() 等。
// 想换元素类型，只要把这里和下面 `impl WallData::new()` 里用到的泛型换掉即可。

pub const COLS:    usize = 12;
pub const COURSES: usize = 10;
pub const LAYERS:  usize = 2;

// ── chunk 划分（第 2 步核心：破坏时只重建局部）──────────────────────
pub const CHUNK_COLS: usize = 4;
pub const CHUNK_ROWS: usize = 5;

pub fn chunks_x() -> usize { COLS.div_ceil(CHUNK_COLS) } // 12/4 = 3
pub fn chunks_y() -> usize { COURSES.div_ceil(CHUNK_ROWS) } // 10/5 = 2

/// 砖 (x,y) 属于哪块 chunk
pub fn chunk_of(x: usize, y: usize) -> (usize, usize) {
    (x / CHUNK_COLS, y / CHUNK_ROWS)
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
///
/// 砖块尺寸完全由 [`Brick`] 元素决定：
/// ```ignore
/// let w = Brick.get_length_x();  // 1.0
/// let h = Brick.get_length_y();  // 0.5
/// let d = Brick.get_length_z();  // 0.6
/// ```
/// running bond 错缝：奇数层偏移半砖宽 —— "砖墙像砖墙"的灵魂。
pub fn block_center(c: usize, y: usize, x: usize) -> Vec3 {
    let bw = Brick::WIDTH;
    let bh = Brick::HEIGHT;
    let bd = Brick::DEPTH;
    let wall_w = COLS as f32 * bw;
    let wall_t = LAYERS as f32 * bd;
    let bond = if y % 2 == 1 { bw * 0.5 } else { 0.0 };
    Vec3::new(
        -wall_w / 2.0 + bw / 2.0 + x as f32 * bw + bond,
        bh / 2.0 + y as f32 * bh,
        -wall_t / 2.0 + bd / 2.0 + c as f32 * bd,
    )
}

/// 世界坐标 → 数据下标（block_center 的逆运算）。
pub fn block_from_point(p: Vec3) -> Option<(usize, usize, usize)> {
    let bw = Brick::WIDTH;
    let bh = Brick::HEIGHT;
    let bd = Brick::DEPTH;
    let wall_w = COLS as f32 * bw;
    let wall_t = LAYERS as f32 * bd;
    if p.x < -wall_w / 2.0 || p.x > wall_w / 2.0 + bw * 0.5 {
        return None;
    }
    if p.z < -wall_t / 2.0 || p.z > wall_t / 2.0 {
        return None;
    }
    let y = (p.y / bh).floor() as usize;
    if y >= COURSES {
        return None;
    }
    let bond = if y % 2 == 1 { bw * 0.5 } else { 0.0 };
    let mut x = ((p.x + wall_w / 2.0 - bond) / bw).floor() as i32;
    x = x.clamp(0, COLS as i32 - 1);
    let c = (((p.z + wall_t / 2.0) / bd).floor() as usize).min(LAYERS - 1);
    Some((c, y, x as usize))
}

// ── 第 2 步：数据 → 合并网格 ────────────────────────────────────────
//
// 这里不再手写 "Cuboid::new(BLOCK_W, BLOCK_H, BLOCK_D)" 和颜色逻辑，
// 直接复用 [`Brick::painted_mesh`] —— 想把墙换成水泥块墙？
// 把下面 3 处 `Brick::painted_mesh`、`Brick::PALETTE`、`Brick::SIZE`
// 替换成 `CementBlock::xxx`，行为保持不变，画面直接变混凝土。

pub fn build_chunk_mesh(wall: &WallData, cx: usize, cy: usize) -> Mesh {
    let x0 = cx * CHUNK_COLS;
    let y0 = cy * CHUNK_ROWS;

    // 所有"活着"的砖：每块一张"带色 + UV 翻转"的 painted_mesh
    let mut parts: Vec<(Mesh, [f32; 4])> = Vec::new();
    for c in 0..LAYERS {
        for y in y0..(y0 + CHUNK_ROWS).min(COURSES) {
            for x in x0..(x0 + CHUNK_COLS).min(COLS) {
                if !wall.alive(c, y, x) {
                    continue;
                }
                let center = block_center(c, y, x);
                // 关键：直接走 Element::painted_mesh 统一工厂
                let brick_mesh = <Brick as Element>::painted_mesh(
                    x as i32, y as i32, c as i32, 0,
                    center, true,
                );
                parts.push((brick_mesh, [1.0, 1.0, 1.0, 1.0])); // 顶点色已 baked 在 mesh 里，merge 乘子用全1
            }
        }
    }

    if parts.is_empty() {
        return empty_triangle_mesh();
    }

    // 合并成一张 chunk 大 mesh，平面法线避免软阴影糊成玩具感
    let mut base = merge_flat(parts);
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}

// ── 验证出口：ASCII 截面 ────────────────────────────────────────────

pub fn print_wall_section(wall: &WallData, c: usize) {
    println!("── 墙截面（厚度层 c={}）──", c);
    for y in (0..COURSES).rev() {
        let mut row = String::new();
        for x in 0..COLS {
            row.push(if wall.alive(c, y, x) { '▓' } else { '·' });
        }
        println!("层{:2}  {}", y, row);
    }
    let total = LAYERS * COLS * COURSES;
    println!("剩余砖块: {}/{}", wall.alive_count(), total);
}
