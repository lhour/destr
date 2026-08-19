# basic_elements_showcase · 场景基础元素陈列

**Binary**: `cargo run --release --bin basic_elements_showcase`

## 效果预览

三个元素（砖 / 水泥块 / 碎砖）按 X 轴排成一列，每块背后一张白底背板作参照：

![showcase_shot](../../shots/basic_elements_showcase_shot.png)

`--demo` 模式的控制台输出（含 API 一致性断言）：

```
· 标准砖         NAME=Brick           SIZE=(1.00, 0.50, 0.60)
· 水泥块         NAME=CementBlock     SIZE=(1.00, 0.60, 0.60)
· 碎砖(基准)      NAME=DebrisPiece     SIZE=(0.22, 0.22, 0.22)
· Brick API 一致性 (SIZE↔get_length / get_length_x / get_width↔_x / get_size↔_length): ✓ PASS
```

## 它解决什么问题

做搭场景 / 可破坏 / 大地图时，最忌讳的就是"用到 Brick 时查一下 BLOCK_W 常量，
用到 Debris 时又凭记忆手写 0.22 这个数字，改一处忘了改另一处"。
本仓库提供了 **`destr::elements::Element` trait**，把所有"基础建材"的参数全封装：

- 尺寸（WIDTH / HEIGHT / DEPTH / SIZE）只写在一处
- 想拿 `getLength(砖)` 这种实例级查询：有 `brick.get_length_x/y/z()` 和别名 `brick.get_width()/get_height()/get_depth()`
- 默认 Mesh / 默认贴图 / 默认材质、三档调色板（带哈希选色）全部是 trait 关联项，一个 `Brick::xxx` 就够，现场不用拼写

想新加入一种元素（路缘石、门槛石、瓦、…）？
在 [`src/elements.rs`](../../src/elements.rs) 里照 `impl Element for CementBlock { ... }` 复制 15 行即可，**所有 case 立即能用**。

## 核心文件

| 文件 | 负责 |
|---|---|
| [`src/elements.rs`](../../src/elements.rs) | 核心：`Element` trait + `Brick` / `CementBlock` / `DebrisPiece` 三实现 + `half_after_scale` / `empty_triangle_mesh` 辅助 |
| [`src/tex.rs`](../../src/tex.rs) | 新增 `cement_texture()`（混凝土面噪点 + 拼缝软阴影）|
| [`src/cases/basic_elements_showcase.rs`](../../src/cases/basic_elements_showcase.rs) | 本 case 入口：排三个元素 + 背板 + --demo API 断言 |
| [`src/common.rs`](../../src/common.rs) | 新增 `spawn_camera_at`（`spawn_default_camera` 的可配置版本） |

## 为什么设计成 trait 而不是一个 struct

两种写法你都能用（见仓库 README 的"基础元素库用法速览"）：

1. **类型级常量**：`Brick::WIDTH`、`CementBlock::SIZE` —— 编译期展开，零开销。
2. **实例方法**：先 `let b = Brick; b.get_length_x()` —— 适合"泛型函数统一处理 N 种元素"的场景，比如墙模块里 `<T as Element>::painted_mesh(...)` 调统一工厂。

trait 方案的关键好处：**新元素加进来，不破坏任何使用 Element API 的 case**。
比如 destructible_wall 里的 `WallAssets` 现在已经注册了 `cement_material`，
之后你想做一个"水泥墙"case，只要把 `wall::build_chunk_mesh` 里的 `Brick::` 改成 `CementBlock::`，
画面立刻从砖变混凝土——不用再跑去找贴图、调色板、mesh 尺寸。

## 运行方式

```bash
# 交互查看
cargo run --release --bin basic_elements_showcase

# 演示模式（截图 + 打印尺寸 + 断言 API 一致 + 退出）
cargo run --release --bin basic_elements_showcase -- --demo
# → shots/basic_elements_showcase_shot.png
```

## 想继续扩展？建议顺序

1. 补新元素：`CurbStone`（路缘石） / `FloorTile1m`（1×1 地砖） / `RoofTile`（瓦片）
2. 每个新元素在本 case 里追加一行 `ITEMS` + match 臂，自动进入陈列
3. 下次新增 case 就从 `Element::default_material` 拉材质，不要再出现"手写 StandardMaterial 字段"的代码了
