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
