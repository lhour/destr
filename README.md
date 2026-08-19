# destr · Bevy 3D 基础场景效果示例仓库

> 中文 / English: 文档以中文为主，面向中文读者；代码中注释中英混合。

`destr` 是一个**示例型（recipe / gallery）仓库**，每个 case 演示一个最小可运行的
Bevy 3D 场景效果（可破坏、可交互、粒子、NPC、流式加载…），
方便你在做自己的项目时**先找对应 case 抄代码，再按需求扩展**。

## 快速开始

```bash
# 列出所有 case
ls src/cases/*.rs | sed 's/src\/cases\///;s/\.rs$//'

# 跑某个 case（示例：可破坏砖墙）
cargo run --release --bin destructible_wall

# 无人值守演示模式（自动操作 + 截图 + 退出，用于 CI / 生成效果图）
cargo run --release --bin destructible_wall -- --demo
# 截图会保存到  shots/<case_name>_shot.png
```

## Case 目录

| # | Binary | 效果 | 关键技术点 |
|---|---|---|---|
| 01 | `destructible_wall` | 可破坏砖墙（左键打洞，碎砖掉落） | 体素数据层、Chunk 网格重建、消息解耦、程序化贴图边框、碎砖手写物理 |
| 02 | `basic_elements_showcase` | 6 种基础元素陈列（砖/水泥块/碎砖/不规则石块/拱曲面楔形砖/歪曲树干 排成一列） | `destr::elements::Element` trait、尺寸 API 封装、三种非立方 Mesh 工具、程序化贴图×5 |
| 03 | `semicircular_arch` | 半圆拱门（12 块 ArchBrick 拼拱圈 + 砖砌支座 + 墩基/门槛） | ArchBrick 自定义弧度 `custom_ring_brick(π/12)`、Y 轴旋转拼装、错缝砌支座 |
| 04 | `small_grove` | 小树林 3×3=9 棵（每棵不同 seed/bend/高度/锥度）+ 每棵脚下 2~5 块 IrregularRock + 22 块 DebrisPiece 地表碎物 | `curved_trunk_mesh` 参数化 + `irregular_rock_mesh` 尺寸/旋转随机 + hash2 确定性随机 |

每个 case 自己的说明请点目录：

- [cases/destructible_wall/README.md](cases/destructible_wall/README.md)
- [cases/basic_elements_showcase/README.md](cases/basic_elements_showcase/README.md)
- [cases/semicircular_arch/README.md](cases/semicircular_arch/README.md)
- [cases/small_grove/README.md](cases/small_grove/README.md)

## 共享库（`destr::*`）

所有 case 通过本 crate 根 `src/lib.rs` 暴露的脚手架 API 复用公共逻辑，
避免每个 case 重写灯光/相机/地面。

| 模块 | 用途 |
|---|---|
| [`destr::common`](src/common.rs) | `add_default_plugins` / `spawn_sun` / `spawn_default_camera` / `spawn_camera_at` / `spawn_ground` / `plain_material` / 网格工具 `merge_flat` `tint` `flip_uv` |
| [`destr::demo`](src/demo.rs) | `DemoDriver` 资源 / `request_screenshot` / `request_exit`（`--demo` 模式约定） |
| [`destr::tex`](src/tex.rs) | 5 张程序化贴图：`brick_texture` `cement_texture` `debris_texture`(无边框碎砖) `rock_texture`(天然岩) `bark_texture`(树干皴裂纹) + `hash2` 整数哈希随机 |
| [`destr::elements`](src/elements.rs) | **场景基础元素库**：`Element` trait + 6 类实现（见下表）+ 3 个手搓非立方 Mesh 工具 |

### 内置 6 种基础元素（`use destr::elements::*`）

| 类型 | 中文名 | 默认 W × H × D | 贴图 | 典型用途 |
|---|---|---|---|---|
| `Brick` | 标准砖 | 1.00 × 0.50 × 0.60 | 砖面（带砂浆边框） | 砌体墙、铺砖、装饰柱 |
| `CementBlock` | 水泥块 | 1.00 × 0.60 × 0.60 | 混凝土（骨料噪+拼缝阴影） | 混凝土墙、立柱、路缘石 |
| `DebrisPiece` | 碎砖(无边框) | 0.22 × 0.22 × 0.22 | 纯脏噪点（**无砂浆无灰缝** ✅） | 破坏掉落物、废墟堆 |
| `IrregularRock` | 不规则石块 | 0.90 × 0.75 × 0.90 | 冷灰天然岩 | 河滩乱石、山脚 |
| `ArchBrick` | 拱曲面楔形砖 | 1.04 × 0.50 × 0.40 | 砂砖暖色 | 拼半圆拱、绕一圈拼圆柱曲面 |
| `CurvedCylinderTrunk` | 歪曲圆柱/树干 | 1.10 × 3.00 × 0.90 | 树皮皴裂纹 | 森林树干、歪曲柱子 |

另外暴露三个"自定义 Mesh 工厂"自由函数（做 case 需要时直接调）：
- `irregular_rock_mesh(size, seed)` — 抖动立方体顶点做多面体
- `arch_brick_mesh(r_outer, thick, arc_rad, height, slices)` — 任意弧度楔形砖
- `curved_trunk_mesh(height, r_base, r_tip, rs, vs, bend, seed)` — 任意锥度/弯曲/扰动的圆柱

### 基础元素库用法速览

```rust
use destr::elements::{
    ArchBrick, Brick, CurvedCylinderTrunk, DebrisPiece, Element, IrregularRock,
};

// 方式一：类型级常量（编译器已知，零开销）
let (w, h, d) = (Brick::WIDTH, Brick::HEIGHT, Brick::DEPTH); // (1.00, 0.50, 0.60)
let palette = DebrisPiece::PALETTE; // 脏灰三档
let s: Vec3 = CurvedCylinderTrunk::SIZE; // (1.10, 3.00, 0.90)

// 方式二：实例方法（像你说的"getLength(砖)"）
let b = Brick;
assert_eq!(b.get_length_x(), 1.0);
assert_eq!(b.get_length_y(), 0.5);
assert_eq!(b.get_length_z(), 0.6);
assert_eq!(b.get_length(), Brick::SIZE);      // 三维合一
assert_eq!(b.get_width(),  b.get_length_x()); // 别名等价
assert_eq!(b.get_height(), b.get_length_y());
assert_eq!(b.get_depth(),  b.get_length_z());

// 方式三：默认 Mesh / 贴图 / 材质 —— 不再手拼 StandardMaterial 字段
let mesh: Mesh = IrregularRock::base_mesh();        // 固定 seed 的不规则石
let mesh2: Mesh = IrregularRock::rock_mesh(42);     // 同一尺寸，换形状
let wedge: Mesh = ArchBrick::base_mesh();            // 30°楔形曲面砖
let trunk: Mesh = CurvedCylinderTrunk::trunk_mesh(3, Vec2::new(0.4, 0.0)); // 朝+X弯的树
let material = Brick::default_material(&mut materials, &mut images, Color::WHITE, 0.95);
let painted  = Brick::painted_mesh(x, y, c, 0, center, true); // 含选色+UV镜像
```

## 统一约定（新建 case 前必读）

1. **二进制命名**：`snake_case`，简短描述效果。例：`floating_island`、`npc_crowd`
2. **入口文件**：`src/cases/<name>.rs`；如果 case 需要私有模块，放 `src/cases/<name>/mod.rs` 子目录
3. **Cargo.toml**：每加一个 case，追加：
   ```toml
   [[bin]]
   name = "<name>"
   path = "src/cases/<name>.rs"
   ```
4. **两种模式必须都能跑**：
   - 交互模式：`cargo run --bin <name>`（玩家能操作）
   - 演示模式：`cargo run --bin <name> -- --demo`（自动操作 + `shots/<name>_shot.png` 截图 + 退出）
5. **演示模式必须验证真实玩家路径**：不要写一条"演示专用捷径"，应该复用玩家事件管道（比如墙 case 的 `BlockPunched` 消息，鼠标和演示共用同一入口）
6. **截图尺寸固定 1280×720**：`common::add_default_plugins` 已经设好，不要改

## 新建 Case（三分钟模板）

1. 复制模板：
   ```bash
   cp src/cases/TEMPLATE.rs src/cases/my_case.rs
   # 如需私有子模块：
   mkdir -p src/cases/my_case && touch src/cases/my_case/mod.rs
   ```
2. 编辑 `Cargo.toml`，加一条 `[[bin]]` 指向新文件
3. 复制 case 文档：
   ```bash
   cp -r cases/destructible_wall cases/my_case
   # 再改 cases/my_case/README.md 里的描述
   ```
4. 按 TODO 注释填逻辑，直到 `cargo run --bin my_case` 能跑
5. 用 `--demo` 模式生成一张效果图，更新 README 里的截图引用
6. 在仓库根 `README.md` 的 Case 目录里补一行

## 技术栈 / 依赖版本

| 项 | 版本 |
|---|---|
| Bevy | 0.18 |
| Rust | 1.92+（`rustc --version`） |
| Cargo.toml 里 `[profile.*]` 的 opt-level 设置 | 已调好，不要改 |

## 常见沙箱/无头环境运行提示

远程沙箱里跑 Bevy（无 GPU、无显示器）需要：

```bash
# 安装一次即可
sudo apt install xvfb mesa-vulkan-drivers libvulkan1 libxkbcommon-x11-0 libwayland-dev libudev-dev libasound2-dev

# 然后用：
Xvfb :99 -screen 0 1280x720x24 &
DISPLAY=:99 WGPU_BACKEND=vulkan VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
  cargo run --release --bin <case_name> -- --demo
```

无头软渲染速度慢是正常的；真机运行时 Bevy 会自动选真实 GPU。
