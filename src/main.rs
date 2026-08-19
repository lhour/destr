//! 第 1 步：体素数据层。
//! 一面墙 = 一张 3D 表格。砖不是实体，是表格里的一个 bool。

use bevy::prelude::*;

// ── 尺寸常量（沿用 chapel 的习惯：禁止魔法数字，全部常量推导）──
pub const BLOCK_W: f32 = 1.0; // 砖宽（X 方向）
pub const BLOCK_H: f32 = 0.5; // 砖高（Y 方向，一皮砖）
pub const BLOCK_D: f32 = 0.6; // 砖厚（Z 方向）
pub const COLS: usize = 12; // 墙有多少列砖
pub const COURSES: usize = 10; // 墙有多少皮（层）砖

/// 一面砖墙的体素数据。
/// `blocks[c][y][x]`：x=列号, y=层数(皮), c=墙厚方向的格子。
/// true = 这块砖还活着；false = 已被打掉。
///
/// 为什么是三维？墙有厚度——外侧一皮、内侧一皮，打穿要打掉两层。
/// 简单版可以只有一维厚度（本教学用 2 层）。
pub struct WallData {
    blocks: Vec<Vec<Vec<bool>>>, // [c][y][x] 索引
}

impl WallData {
    /// 建一面完整的新墙。
    pub fn new() -> Self {
        let blocks = vec![vec![vec![true; COLS]; COURSES]; 2]; // 厚度 2 皮
        WallData { blocks }
    }

    /// 这块砖活着吗？（越界视为不存在 → 返回 false）
    pub fn alive(&self, c: usize, y: usize, x: usize) -> bool {
        *self.blocks
            .get(c)
            .and_then(|layer| layer.get(y))
            .and_then(|row| row.get(x))
            .unwrap_or(&false)
    }

    /// 打掉一块砖：唯一需要"写"的操作。
    pub fn destroy(&mut self, c: usize, y: usize, x: usize) {
        if self.alive(c, y, x) {
            self.blocks[c][y][x] = false;
        }
    }

    /// 还剩多少块砖（教学用：打印统计）。
    pub fn alive_count(&self) -> usize {
        self.blocks
            .iter()
            .flat_map(|layer| layer.iter())
            .flat_map(|row| row.iter())
            .filter(|&&b| b)
            .count()
    }

    /// 把这面墙打成"十字洞"——第 2 步之前先肉眼验证数据层是对的。
    /// （临时教学函数，第 3 步会换成鼠标点击来调 destroy）
    pub fn punch_test_hole(&mut self) {
        for c in 0..2 {
            for y in 3..7 {
                for x in 4..8 {
                    self.destroy(c, y, x);
                }
            }
        }
    }
}

/// 把墙体数据打印成 ASCII 截面（就是 SCENE_SKILL.md 里的"内存截面扫描"！）
/// 无需渲染就能验证数据正确性。
pub fn print_wall_section(wall: &WallData, c: usize) {
    println!("── 墙截面（厚度层 c={}）──", c);
    for y in (0..COURSES).rev() {
        // y 从高往低打印（层 9 在最上面，符合直觉）
        let mut row = String::new();
        for x in 0..COLS {
            row.push(if wall.alive(c, y, x) { '▓' } else { '·' });
        }
        println!("层{:2}  {}", y, row);
    }
    println!("剩余砖块: {}/{}", wall.alive_count(), 2 * COLS * COURSES);
}

fn main() {
    let mut wall = WallData::new();
    println!("=== 新墙 ===");
    print_wall_section(&wall, 0);

    wall.punch_test_hole();
    println!("\n=== 打了一个十字洞之后 ===");
    print_wall_section(&wall, 0);

    // 这一步完全不需要 Bevy 渲染——数据层可以独立测试。
    // App::new() 等第 2 步再加。
    let _ = App::new(); // 占位，保持 bevy 依赖被使用（第 2 步会用）
}
