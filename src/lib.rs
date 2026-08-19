//! destr — Bevy 3D 基础场景效果示例共享库。
//!
//! 每个示例（case）是独立的 binary（`src/cases/*.rs`），
//! 通过调用本 crate 暴露的脚手架 API：
//!   - `destr::common::*`：公共灯光、相机、地面、材质、窗口配置
//!   - `destr::demo::*`：`--demo` 无人值守模式（自动操作 + 截图 + 退出）
//!   - `destr::tex::*`：程序化纹理生成工具（砖面等）
//!
//! 新建 case 的最短路径：
//!   1. `cp src/cases/TEMPLATE.rs src/cases/my_case.rs`
//!   2. 在 Cargo.toml 里加一条 `[[bin]]` 指向它
//!   3. 实现 `Case` trait，`main()` 调用 `run_case::<MyCase>()`

pub mod common;
pub mod demo;
pub mod tex;
