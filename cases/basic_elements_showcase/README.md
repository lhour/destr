# basic_elements_showcase · 基础元素陈列（6 类）

**Binary**: `cargo run --release --bin basic_elements_showcase`

## 效果预览

6 种元素从左到右排成一行：Brick → CementBlock → DebrisPiece → IrregularRock → ArchBrick → CurvedCylinderTrunk。
每块元素后立一块 1.3×1.3 白底背板作参照物。

![showcase_shot](../../shots/basic_elements_showcase_shot.png)

## 本轮新增（对照用户需求逐条）

| # | 用户需求 | 实现位置 |
|---|---|---|
| 1 | **碎砖不该有边框** | [`DebrisPiece::default_image()`](../../src/elements.rs#L223-L233) 从 `brick_texture` 改成新增的 `debris_texture()`；新贴图是纯多尺度噪点 + 10% 随机小块暗斑，完全没有几何化砂浆/灰缝 |
| 2 | **生成一些不规则石块** | `IrregularRock` 元素：`irregular_rock_mesh(size, seed)` 把立方体 8 顶点按 seed 做 ±18% 随机位移（[elements.rs L265-L329](../../src/elements.rs#L265-L329)）；贴图用冷岩色 `rock_texture()`（颗粒+四周稍暗，天然岩石感） |
| 3 | **生成圆拱 / 圆柱的曲面砖块** | `ArchBrick` 元素：`arch_brick_mesh(r_outer, thick, arc_rad, height, slices)` 手搓一个楔形曲面砖（外弧圆柱面 + 内弧面 + 顶底 + 两端面，共 6 组面）；默认 30°/弧厚 0.4；想拼半圆拱就拿 12 块 `arc_rad = π/12` 的各自绕 +Y 轴旋转 `k·15°` 即可；想拼整圆柱就 `arc_rad = 2π/N` 绕一圈 |
| 4 | **歪曲的圆柱做树干** | `CurvedCylinderTrunk` 元素：`curved_trunk_mesh(...)` 做 18×7 的分段圆柱，带锥度（底 0.30→顶 0.12）、半径 ±14% 顶点抖动、根部直立→顶部按抛物线 `curve=t·(2-t)` 弯到 `bend=(0.25, 0.15)`；贴图是纵向皴裂纹 `bark_texture()`，棕褐三档调色板；需要做小树林就 `CurvedCylinderTrunk::trunk_mesh(seed, bend)` 传不同 seed + bend |

## 三个 Mesh 工厂（做 case 需要时直接调）

```rust
use destr::elements::{
    arch_brick_mesh, curved_trunk_mesh, half_after_scale, irregular_rock_mesh,
};

// 一块 45°楔形砖（拱的更大片段），高 0.6，厚 0.5
let wedge = arch_brick_mesh(
    2.5,                      // 外半径
    0.5,                      // 壁厚
    std::f32::consts::PI / 4.0, // 45°
    0.6,                      // 高度
    10,                       // 弧度方向分片
);

// 一块更大/更小的自定义尺寸石块
let big_boulder = irregular_rock_mesh(Vec3::new(1.6, 1.1, 1.4), 7);

// 做一棵"朝 +Z 弯得更夸张、锥度更大"的树
let bent_tree = curved_trunk_mesh(
    4.5, 0.38, 0.08,   // 高/底径/顶径
    24, 10,            // 径向/轴向分段（越高越要多）
    Vec2::new(0.05, 0.9),
    11,                // seed
);
```

## Console 输出（--demo）

```
· 标准砖             NAME=Brick                   SIZE=(1.00, 0.50, 0.60)
· 水泥块             NAME=CementBlock             SIZE=(1.00, 0.60, 0.60)
· 碎砖(无边框)        NAME=DebrisPiece             SIZE=(0.22, 0.22, 0.22)
· 不规则石块          NAME=IrregularRock           SIZE=(0.90, 0.75, 0.90)
· 拱曲面楔形砖         NAME=ArchBrick               SIZE=(1.04, 0.50, 0.40)
· 歪曲圆柱树干         NAME=CurvedCylinderTrunk     SIZE=(1.10, 3.00, 0.90)
· 6 类 Element API 一致性: ✓ PASS
```

## 运行方式

```bash
cargo run --release --bin basic_elements_showcase
cargo run --release --bin basic_elements_showcase -- --demo
# → 生成 shots/basic_elements_showcase_shot.png（6 元素全景）
```

## 下一步建议

- 做一个 **case #3：半圆拱门**，把 12 块 ArchBrick 拼起来 + 两侧用 Brick 砌支座，能展示"曲面砖作为构造单元"；
- 做一个 **case #4：废墟乱石堆**，撒 N 个不同 seed 的 IrregularRock，底下散一批 DebrisPiece 当碎渣；
- 做一个 **case #5：小树林**，一行 N 个不同 seed + bend 的 CurvedCylinderTrunk，底部塞几块 IrregularRock 当树根石。
