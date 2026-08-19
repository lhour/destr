//! 程序化纹理工具：跨 case 复用的贴图生成。
//!
//! 所有贴图都在运行时以代码生成——不依赖任何外部图片资源，
//! `cargo build` 完就能直接跑。想换成美术贴图只需替换 asset 路径。

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

// ── 工具：整数哈希 [0,1] 伪随机 ─────────────────────────────────────

pub fn hash2(mut a: i32, mut b: i32) -> f32 {
    // xorshift 风格整数哈希，然后归一化。速度快，够用。
    a ^= b.wrapping_mul(0x45d9_f3b3);
    b ^= a.wrapping_mul(0x53a0_5215);
    a ^= b.wrapping_mul(0x45d9_f3b3);
    let u = ((a as u32).wrapping_mul(2654_435761)) | 0x0080_0000;
    (u >> 8) as f32 / 16_777_216.0 // [0,1)
}

// ── 砖面贴图：边框 + 砂浆 + 倒角 + 噪点 ─────────────────────────────
//
// 用法：
//   let tex = images.add(destr::tex::brick_texture());
//   let mat = materials.add(StandardMaterial { base_color_texture: Some(tex), .. });
//
// 设计约束：贴图是"亮度 mask"（白底，乘法变暗），
// 所以最终颜色 = 顶点色(每砖石色) × 贴图(边框/缝/细节) × 光照。
// 这样每 case 换调色板只需改顶点色表，贴图一张通用。

const BRICK_MORTAR_U: f32 = 0.030; // 竖缝宽（u 方向）
const BRICK_MORTAR_V: f32 = 0.050; // 横缝宽（v 方向，短边稍宽补偿比例）
const BRICK_CHAMFER: f32 = 0.10;   // 倒角过渡带宽（从砂浆边向内）

pub fn brick_texture() -> Image {
    const S: u32 = 256;
    let mut data = Vec::with_capacity((S * S * 4) as usize);
    for py in 0..S {
        for px in 0..S {
            // 到四条边的距离
            let du = (px as f32 / S as f32).min(1.0 - px as f32 / S as f32);
            let dv = (py as f32 / S as f32).min(1.0 - py as f32 / S as f32);
            let lum = if du < BRICK_MORTAR_U || dv < BRICK_MORTAR_V {
                // 砂浆缝：基本灰 + 轻噪（缝要平，噪点减半）
                0.62 + (hash2(px as i32, py as i32 * 7) - 0.5) * 0.06
            } else {
                // 砖面：靠缝暗 → 中心亮，叠两档频率噪点
                let d = ((du - BRICK_MORTAR_U).min(dv - BRICK_MORTAR_V) / BRICK_CHAMFER)
                    .clamp(0.0, 1.0);
                0.78 + 0.22 * d
                    + (hash2(px as i32 * 3, py as i32 * 11) - 0.5) * 0.09
                    + (hash2(px as i32 / 21, py as i32 / 17) - 0.5) * 0.08
            };
            let b = (lum.clamp(0.0, 1.0) * 255.0) as u8;
            data.extend_from_slice(&[b, b, b, 255]);
        }
    }
    Image::new(
        Extent3d { width: S, height: S, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

// ── 水泥面贴图：无砂浆，颗粒粗、带大起伏 ─────────────────────────────
//
// 和砖面贴图一样是"亮度 mask"，用于顶点色 × 贴图的乘法着色。
// 特性：四边 0.5% 的软阴影（让相邻混凝土块之间有"拼缝阴影"感），
//       主体用高频 + 低频噪点卷积模拟骨料颗粒。

pub fn cement_texture() -> Image {
    const S: u32 = 256;
    let mut data = Vec::with_capacity((S * S * 4) as usize);
    for py in 0..S {
        for px in 0..S {
            // 拼缝软阴影：四边到中心的距离按 6% 衰减
            let du = (px as f32 / S as f32).min(1.0 - px as f32 / S as f32);
            let dv = (py as f32 / S as f32).min(1.0 - py as f32 / S as f32);
            let edge = (du.min(dv) / 0.06).clamp(0.0, 1.0);
            let shadow = 0.82 + 0.18 * edge;
            // 主体：三档频率混合的噪点，模拟水泥 + 骨料 + 砂
            let n1 = (hash2(px as i32,        py as i32)        - 0.5) * 0.10;
            let n2 = (hash2(px as i32 / 3,    py as i32 / 3)    - 0.5) * 0.12;
            let n3 = (hash2(px as i32 / 19,   py as i32 / 17)   - 0.5) * 0.08;
            let base = 0.80 + n1 + n2 + n3;
            let lum = base * shadow;
            let b = (lum.clamp(0.0, 1.0) * 255.0) as u8;
            data.extend_from_slice(&[b, b, b, 255]);
        }
    }
    Image::new(
        Extent3d { width: S, height: S, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

// ── 碎砖面贴图：无砂浆缝 / 无边框，纯脏噪点 ─────────────────────────
//
// 用户明确要求："碎砖不该有边框"。只做多尺度颗粒 + 10% 随机小块暗斑。
pub fn debris_texture() -> Image {
    const S: u32 = 128;
    let mut data = Vec::with_capacity((S * S * 4) as usize);
    for py in 0..S {
        for px in 0..S {
            let n1 = (hash2(px as i32,        py as i32)        - 0.5) * 0.18;
            let n2 = (hash2(px as i32 / 2,    py as i32 / 2)    - 0.5) * 0.14;
            let n3 = (hash2(px as i32 / 11,   py as i32 / 9)    - 0.5) * 0.10;
            let chip = if hash2(px as i32 / 4, py as i32 / 4) > 0.90 { -0.08 } else { 0.0 };
            let lum = 0.66 + n1 + n2 + n3 + chip;
            let b = (lum.clamp(0.0, 1.0) * 255.0) as u8;
            data.extend_from_slice(&[b, b, b, 255]);
        }
    }
    Image::new(
        Extent3d { width: S, height: S, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

// ── 石块面贴图：冷色天然岩，低频起伏 + 颗粒 + 四周稍暗 ─────────────
pub fn rock_texture() -> Image {
    const S: u32 = 256;
    let mut data = Vec::with_capacity((S * S * 4) as usize);
    for py in 0..S {
        for px in 0..S {
            let du = (px as f32 + 7.0) / S as f32;
            let dv = (py as f32 + 3.0) / S as f32;
            let low = (du * 3.3 + dv * 2.1).sin() * 0.06
                    + ((du + dv) * 5.7).cos() * 0.04;
            let n1 = (hash2(px as i32,      py as i32)      - 0.5) * 0.10;
            let n2 = (hash2(px as i32 / 5,  py as i32 / 4)  - 0.5) * 0.10;
            let n3 = (hash2(px as i32 / 29, py as i32 / 31) - 0.5) * 0.08;
            let ed = (du.min(1.0 - du) * dv.min(1.0 - dv) * 20.0).min(1.0);
            let vignette = 0.86 + 0.14 * ed;
            let lum = (0.70 + low + n1 + n2 + n3) * vignette;
            let b = (lum.clamp(0.0, 1.0) * 255.0) as u8;
            data.extend_from_slice(&[b, b, b, 255]);
        }
    }
    Image::new(
        Extent3d { width: S, height: S, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

// ── 树皮贴图：纵向皴裂纹 + 少量横向断层 + 颗粒 ───────────────────────
pub fn bark_texture() -> Image {
    const S: u32 = 256;
    let mut data = Vec::with_capacity((S * S * 4) as usize);
    for py in 0..S {
        for px in 0..S {
            let px_f = px as f32;
            let py_f = py as f32;
            let wave = (px_f * 0.11 + (py_f * 0.05).sin() * 9.0).sin();
            let ridge = (wave.abs() - 0.4).max(0.0) * 2.5;
            let vert = ridge * 0.20;
            let horiz = if (py_f / 29.0).fract() < 0.06 { -0.10 } else { 0.0 };
            let g = (hash2(px as i32, py as i32) - 0.5) * 0.08;
            let lum = 0.58 - vert + horiz + g;
            let b = (lum.clamp(0.0, 1.0) * 255.0) as u8;
            data.extend_from_slice(&[b, b, b, 255]);
        }
    }
    Image::new(
        Extent3d { width: S, height: S, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}
