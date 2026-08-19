//! 公共场景脚手架：所有 case 都可以复用的灯光/相机/地面/窗口/材质。
//!
//! 这些是"可参考但不必照搬"的示例默认值——case 觉得不适合直接替换即可。

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Mesh, PrimitiveTopology};
use bevy::prelude::*;
use bevy::window::WindowResolution;

/// 默认窗口分辨率（16:9，截图统一尺寸）
pub const WIN_W: u32 = 1280;
pub const WIN_H: u32 = 720;

/// 天空色（淡蓝，避免 case 之间天空风格打架）
pub const SKY: Color = Color::srgb(0.68, 0.80, 0.92);

// ── App 层的一次性配置（放到 main 里调用）──────────────────────────

/// 默认插件 + 共享窗口设置。所有 case 的 `main` 应从这个函数开始。
pub fn add_default_plugins(app: &mut App, title: impl Into<String>) {
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: title.into(),
            resolution: WindowResolution::new(WIN_W, WIN_H),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(SKY))
    .insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.62, 0.68, 0.8),
        brightness: 900.0,
        affects_lightmapped_meshes: true,
    });
}

// ── 场景层公共对象（Startup 里 spawn）───────────────────────────────

/// 暖阳光 + 硬阴影。这是所有室外场景的默认光照。
pub fn spawn_sun(commands: &mut Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 40_000.0,
            color: Color::srgb(1.0, 0.96, 0.88),
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, -0.6, -0.2)),
    ));
}

/// 3/4 透视观察相机：看 (0, 2, 0) 正前方。
pub fn spawn_default_camera(commands: &mut Commands) {
    spawn_camera_at(commands, Vec3::new(9.0, 4.5, 14.0), Vec3::new(0.0, 2.0, 0.0));
}

/// 3/4 透视观察相机（自定义眼点 / 看向目标）。
///
/// 典型用例：展示小型陈列场景时把相机拉近；展示大墙场景时拉远。
pub fn spawn_camera_at(commands: &mut Commands, eye: Vec3, look_at: Vec3) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(eye).looking_at(look_at, Vec3::Y),
        bevy::camera::Hdr,
    ));
}

/// 60×60 土色地面，不带阴影投射器（避免地面被自己的影子染得漆黑）。
/// 颜色写在顶点色里，所以任何"白底 × 顶点色"的材质都能显示。
pub fn spawn_ground(commands: &mut Commands, meshes: &mut Assets<Mesh>, mat: Handle<StandardMaterial>) {
    let mut g = Cuboid::new(60.0, 0.4, 60.0).mesh().build();
    g = g.translated_by(Vec3::new(0.0, -0.2, 0.0));
    let n = g.count_vertices();
    let soil = Color::srgb_u8(0x6b, 0x6b, 0x5e).to_linear();
    g.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[soil.red, soil.green, soil.blue, 1.0]; n]);
    g.duplicate_vertices();
    g.compute_flat_normals();
    commands.spawn((
        Mesh3d(meshes.add(g)),
        MeshMaterial3d(mat),
        Transform::default(),
        NotShadowCaster,
    ));
}

/// 共享"素面白材质"：地面、碎砖之类不需要贴图的物体用这个。
pub fn plain_material(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        ..default()
    })
}

// ── 通用网格工具（跨 case 可能复用）─────────────────────────────────

/// 合并多个小网格 + 顶点色：
///   - 所有 mesh 必须已经有 Position/Normal/UV/Color 四个属性
///   - 合并成一个 TriangleList 大网格（扁平着色）
/// 用途：chunk 合并、墙体拼装、程序化道具拼装。
pub fn merge_flat(parts: Vec<(Mesh, [f32; 4])>) -> Mesh {
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut norm: Vec<[f32; 3]> = Vec::new();
    let mut uv: Vec<[f32; 2]> = Vec::new();
    let mut col: Vec<[f32; 4]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    for (m, tint) in parts {
        let base = pos.len() as u32;
        let Some(p) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { continue };
        let Some(n) = m.attribute(Mesh::ATTRIBUTE_NORMAL) else { continue };
        let Some(u) = m.attribute(Mesh::ATTRIBUTE_UV_0) else { continue };
        use bevy::mesh::VertexAttributeValues::*;
        if let (Float32x3(p), Float32x3(n), Float32x2(u)) = (p, n, u) {
            let count = p.len();
            pos.extend(p.iter().copied());
            norm.extend(n.iter().copied());
            uv.extend(u.iter().copied());
            col.extend(std::iter::repeat(tint).take(count));
            if let Some(Float32x4(c)) = m.attribute(Mesh::ATTRIBUTE_COLOR) {
                // 如果原 mesh 自己带顶点色，就和 tint 相乘（两个机制互补）
                for i in 0..count {
                    let (a, b) = (c[i], tint);
                    col[base as usize + i] =
                        [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]];
                }
            }
            match m.indices().unwrap() {
                bevy::mesh::Indices::U32(v) => idx.extend(v.iter().map(|&i| i + base)),
                _ => panic!("merge_flat only supports U32 indices"),
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, norm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(bevy::mesh::Indices::U32(idx))
}

/// 给 mesh 每个顶点写同一个颜色。
pub fn tint(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

/// 镜像 UV（u→1-u / v→1-v）：让同一张贴图有 4 种朝向，避免纹理重复感。
pub fn flip_uv(mut m: Mesh, fx: bool, fy: bool) -> Mesh {
    if !fx && !fy {
        return m;
    }
    use bevy::mesh::VertexAttributeValues::Float32x2;
    if let Some(Float32x2(uvs)) = m.attribute(Mesh::ATTRIBUTE_UV_0) {
        let flipped: Vec<[f32; 2]> = uvs
            .iter()
            .map(|[u, v]| [if fx { 1.0 - u } else { *u }, if fy { 1.0 - v } else { *v }])
            .collect();
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, flipped);
    }
    m
}

/// 对 UV 做仿射：`(u, v) → (u * su + du, v * sv + dv)`。
///
/// 典型用法：
///   - 让每棵树的树皮花纹密度不同：scale_uv(m, rng(0.7, 1.4), rng(0.6, 1.6))
///   - 让花纹沿树干高度错开：translate_uv(m, 0, rng(0, 1))
///   - 两者组合：先 scale 再 translate（顺序：先缩后移更符合直觉）。
pub fn transform_uv(mut m: Mesh, su: f32, sv: f32, du: f32, dv: f32) -> Mesh {
    use bevy::mesh::VertexAttributeValues::Float32x2;
    if let Some(Float32x2(uvs)) = m.attribute(Mesh::ATTRIBUTE_UV_0) {
        let tx: Vec<[f32; 2]> = uvs
            .iter()
            .map(|[u, v]| [u * su + du, v * sv + dv])
            .collect();
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, tx);
    }
    m
}

/// 便捷函数：只缩放 UV（等价 `transform_uv(m, su, sv, 0.0, 0.0)`）。
pub fn scale_uv(m: Mesh, su: f32, sv: f32) -> Mesh { transform_uv(m, su, sv, 0.0, 0.0) }

/// 便捷函数：只平移 UV（等价 `transform_uv(m, 1.0, 1.0, du, dv)`）。
pub fn translate_uv(m: Mesh, du: f32, dv: f32) -> Mesh { transform_uv(m, 1.0, 1.0, du, dv) }

// ─────────────────────────────────────────────────────────────────
//  JSON 场景描述语言（Scene Description Language，SDL）
//
//  思路：你写一份 JSON 描述"由哪些图元、按什么坐标/公式拼起来"，
//  程序加载 JSON → 自动算出每个图元的尺寸/角度/位置 → 合并成单个 Mesh。
//
//  完整 JSON 示例见 assets/json/car_body.json。
// ─────────────────────────────────────────────────────────────────

use serde::Deserialize;
use std::collections::HashMap;

/// 一个图元（Cuboid / Cylinder 等）的描述。可以放在 parts[] 里。
#[derive(Debug, Deserialize, Clone)]
pub struct PartDesc {
    /// 唯一 id（可选，方便调试打印）
    #[serde(default)]
    pub id: String,
    /// 图元类型："cuboid" / "cylinder" / "cone" / "capsule"
    pub r#type: String,
    /// 尺寸表达式（按类型）：
    ///   cuboid:   [w, h, l]
    ///   cylinder: [radius, length]
    ///   cone:     [radius, length]
    ///   capsule:  [radius, length]
    pub size: Vec<Expr>,
    /// 绕 X/Y/Z 轴旋转角（弧度表达式）。顺序：先 Z → 再 Y → 再 X
    #[serde(default)]
    pub rotate_x: Option<Expr>,
    #[serde(default)]
    pub rotate_y: Option<Expr>,
    #[serde(default)]
    pub rotate_z: Option<Expr>,
    /// 平移坐标 [x, y, z]（表达式）
    #[serde(default)]
    pub translate: Vec<Expr>,
    /// 颜色：十六进制字符串，如 "0xb5312a" / "#b5312a"
    #[serde(default = "default_color")]
    pub color: String,
    /// 细分（仅 cylinder/cone/capsule 生效，默认 16）
    #[serde(default)]
    pub resolution: Option<u32>,
}

fn default_color() -> String { "#c0c0c0".to_string() }

/// 一个表达式：支持 "常量名"、"锚点名.y/z"、数字字符串、四则运算、sqrt、PI
/// 我们用一个简单的"先字符串替换 → 再 eval"的小解析器实现。
/// 既足够写坐标公式，又不引入额外依赖。
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum Expr {
    /// 纯数字（f32）
    Num(f32),
    /// 字符串表达式，比如 "CH * 2.00" 或 "(B_top.y + A_top.y) / 2 - 0.01"
    Str(String),
}

/// 整个场景文件的顶层结构
#[derive(Debug, Deserialize, Clone)]
pub struct SceneFile {
    pub version: String,
    /// 命名常量：{ "CW": 0.87, "CH": 0.55, "CL": 2.25 }
    #[serde(default)]
    pub constants: HashMap<String, f32>,
    /// 命名锚点（2D，只含 y/z）：
    ///   { "A_top": { "y": "CH * 2.00", "z": "0.00" } }
    /// 表达式里可写 `A_top.y`、`B_top.z`
    #[serde(default)]
    pub anchors: HashMap<String, Anchor2>,
    /// 图元列表
    #[serde(default)]
    pub parts: Vec<PartDesc>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Anchor2 {
    pub y: Expr,
    pub z: Expr,
}

/// 一个小的表达式求值器：支持 + - * / ()、sqrt(x)、atan2(y,x)、sin/cos、PI
/// 以及变量（常量名）和 `Anchor.y` / `Anchor.z` 形式的锚点属性引用。
struct ExprEnv<'a> {
    constants: &'a HashMap<String, f32>,
    anchors:   &'a HashMap<String, Anchor2>,
    // anchors 的求值结果（表达式求值过程中缓存）
    anchor_vals: HashMap<String, (f32, f32)>,
}

impl<'a> ExprEnv<'a> {
    fn new(c: &'a HashMap<String, f32>, a: &'a HashMap<String, Anchor2>) -> Self {
        Self { constants: c, anchors: a, anchor_vals: HashMap::new() }
    }

    /// 解析一个 Expr → f32
    fn eval(&mut self, e: &Expr) -> Result<f32, String> {
        match e {
            Expr::Num(x) => Ok(*x),
            Expr::Str(s) => self.eval_str(s),
        }
    }

    fn eval_str(&mut self, expr: &str) -> Result<f32, String> {
        // 先替换所有变量和锚点为纯数字，然后递归下降解析表达式
        let preprocessed = self.substitute(expr)?;
        parse_expr(&preprocessed)
    }

    /// 把变量/锚点/PI 替换成数字字面量
    fn substitute(&mut self, s: &str) -> Result<String, String> {
        let mut out = String::with_capacity(s.len());
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c.is_ascii_alphabetic() || c == '_' {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                // 锚点属性形式：AnchorName.y 或 AnchorName.z？
                if i + 1 < chars.len() && chars[i] == '.' {
                    let attr = chars[i + 1];
                    if attr == 'y' || attr == 'z' {
                        let (y, z) = self.resolve_anchor(&name)?;
                        out.push_str(&match attr {
                            'y' => format!("({})", y),
                            _   => format!("({})", z),
                        });
                        i += 2;
                        continue;
                    }
                }
                // 特殊常数
                if name == "PI" {
                    out.push_str(&format!("({})", std::f32::consts::PI));
                    continue;
                }
                if name == "sqrt" || name == "atan2" || name == "sin" || name == "cos" || name == "tan" {
                    // 不替换，直接作为函数名保留
                    out.push_str(&name);
                    continue;
                }
                // 普通命名常量
                if let Some(v) = self.constants.get(&name) {
                    out.push_str(&format!("({})", v));
                    continue;
                }
                return Err(format!("未知标识符: {}", name));
            } else {
                out.push(c);
                i += 1;
            }
        }
        Ok(out)
    }

    fn resolve_anchor(&mut self, name: &str) -> Result<(f32, f32), String> {
        if let Some(v) = self.anchor_vals.get(name).copied() {
            return Ok(v);
        }
        let a = self.anchors.get(name).ok_or_else(|| format!("未知锚点: {}", name))?.clone();
        let y = self.eval(&a.y)?;
        let z = self.eval(&a.z)?;
        self.anchor_vals.insert(name.to_string(), (y, z));
        Ok((y, z))
    }
}

// ── 简单递归下降表达式解析器（+ - * / ()、一元 -、函数 sqrt/sin/cos/tan/atan2） ──
fn parse_expr(s: &str) -> Result<f32, String> {
    let tokens = tokenize(s)?;
    let mut p = Parser { tokens: &tokens, pos: 0 };
    let v = p.parse_add()?;
    if p.pos != p.tokens.len() {
        return Err(format!("表达式解析到末尾时仍有余量：pos={}, tokens={:?}", p.pos, &p.tokens[p.pos..]));
    }
    Ok(v)
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f32),
    Op(char),
    LPar, RPar,
    Comma,
    Ident(String),
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() { i += 1; continue; }
        if c.is_ascii_digit() || (c == '.' && i + 1 < chars.len() && chars[i+1].is_ascii_digit()) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') { i += 1; }
            let num_s: String = chars[start..i].iter().collect();
            let n = num_s.parse::<f32>().map_err(|_| format!("数字解析失败: {}", num_s))?;
            out.push(Tok::Num(n));
            continue;
        }
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphanumeric() { i += 1; }
            let name: String = chars[start..i].iter().collect();
            out.push(Tok::Ident(name));
            continue;
        }
        match c {
            '(' => out.push(Tok::LPar),
            ')' => out.push(Tok::RPar),
            ',' => out.push(Tok::Comma),
            '+' | '-' | '*' | '/' | '^' => out.push(Tok::Op(c)),
            _   => return Err(format!("非法字符: '{}' at pos {}", c, i)),
        }
        i += 1;
    }
    Ok(out)
}

struct Parser<'a> { tokens: &'a [Tok], pos: usize }

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> { self.tokens.get(self.pos) }
    fn bump(&mut self) { self.pos += 1; }

    fn parse_add(&mut self) -> Result<f32, String> {
        let mut lhs = self.parse_mul()?;
        loop {
            match self.peek() {
                Some(Tok::Op('+')) => { self.bump(); lhs += self.parse_mul()?; }
                Some(Tok::Op('-')) => { self.bump(); lhs -= self.parse_mul()?; }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<f32, String> {
        let mut lhs = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(Tok::Op('*')) => { self.bump(); lhs *= self.parse_unary()?; }
                Some(Tok::Op('/')) => { self.bump();
                    let r = self.parse_unary()?;
                    if r.abs() < 1e-12 { return Err("除零".into()); }
                    lhs /= r;
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<f32, String> {
        match self.peek() {
            Some(Tok::Op('-')) => { self.bump(); Ok(-self.parse_atom()?) }
            Some(Tok::Op('+')) => { self.bump(); self.parse_atom() }
            _ => self.parse_atom(),
        }
    }

    fn parse_atom(&mut self) -> Result<f32, String> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => { self.bump(); Ok(n) }
            Some(Tok::LPar) => {
                self.bump();
                let v = self.parse_add()?;
                match self.peek() {
                    Some(Tok::RPar) => { self.bump(); Ok(v) }
                    _ => Err("缺少右括号".into()),
                }
            }
            Some(Tok::Ident(name)) => {
                self.bump();
                // 函数调用形式：Ident(...)
                match self.peek() {
                    Some(Tok::LPar) => {
                        self.bump();
                        let arg1 = self.parse_add()?;
                        let arg2 = if matches!(self.peek(), Some(Tok::Comma)) {
                            self.bump();
                            Some(self.parse_add()?)
                        } else { None };
                        match self.peek() {
                            Some(Tok::RPar) => self.bump(),
                            _ => return Err(format!("函数 {}() 缺少右括号", name)),
                        }
                        call_fn(&name, arg1, arg2)
                    }
                    _ => Err(format!("标识符 {} 不是常量/锚点（未替换）", name)),
                }
            }
            other => Err(format!("期望数字/括号，得到 {:?}", other)),
        }
    }
}

fn call_fn(name: &str, a: f32, b: Option<f32>) -> Result<f32, String> {
    match name {
        "sqrt"  => Ok(a.sqrt()),
        "sin"   => Ok(a.sin()),
        "cos"   => Ok(a.cos()),
        "tan"   => Ok(a.tan()),
        "atan2" => {
            let by = b.ok_or_else(|| "atan2 需要两个参数".to_string())?;
            Ok(a.atan2(by))
        }
        "abs"   => Ok(a.abs()),
        "min"   => {
            let by = b.ok_or_else(|| "min 需要两个参数".to_string())?;
            Ok(a.min(by))
        }
        "max"   => {
            let by = b.ok_or_else(|| "max 需要两个参数".to_string())?;
            Ok(a.max(by))
        }
        _ => Err(format!("未知函数: {}", name)),
    }
}

// ── 顶层入口：加载 JSON 并构建 (merged_mesh, vec[(局部 mesh, tint)]） ─

/// 从 JSON 字符串构建一组 (Mesh, tint)，和手写的 shell_parts 格式完全一致。
pub fn build_parts_from_json(json_str: &str) -> Result<Vec<(Mesh, [f32; 4])>, String> {
    let scene: SceneFile = serde_json::from_str(json_str)
        .map_err(|e| format!("JSON 解析失败: {}", e))?;
    build_parts_from_scene(&scene)
}

/// 从 SceneFile 构建 (Mesh, tint)。和手写 shell_parts 格式一致。
pub fn build_parts_from_scene(scene: &SceneFile) -> Result<Vec<(Mesh, [f32; 4])>, String> {
    let mut env = ExprEnv::new(&scene.constants, &scene.anchors);
    let mut out: Vec<(Mesh, [f32; 4])> = Vec::with_capacity(scene.parts.len());
    for part in &scene.parts {
        let mesh = build_part(part, &mut env)?;
        let tint = color_from_str(&part.color).unwrap_or([0.8, 0.8, 0.8, 1.0]);
        out.push((mesh, tint));
    }
    Ok(out)
}

fn build_part(part: &PartDesc, env: &mut ExprEnv) -> Result<Mesh, String> {
    // 1. 根据 type + size 构造基础 mesh
    let mesh = match part.r#type.as_str() {
        "cuboid" => {
            if part.size.len() != 3 { return Err(format!("cuboid size 需要 3 个元素，got {}", part.size.len())); }
            let w = env.eval(&part.size[0])?;
            let h = env.eval(&part.size[1])?;
            let l = env.eval(&part.size[2])?;
            Cuboid::new(w, h, l).mesh().build()
        }
        "cylinder" => {
            if part.size.len() != 2 { return Err(format!("cylinder size 需要 2 个元素，got {}", part.size.len())); }
            let r = env.eval(&part.size[0])?;
            let len = env.eval(&part.size[1])?;
            Cylinder::new(r, len).mesh().build()
        }
        "cone" => {
            if part.size.len() != 2 { return Err("cone size 需要 2 个元素".into()); }
            let r = env.eval(&part.size[0])?;
            let len = env.eval(&part.size[1])?;
            Cone::new(r, len).mesh().build()
        }
        "capsule" => {
            if part.size.len() != 2 { return Err("capsule size 需要 2 个元素".into()); }
            let r = env.eval(&part.size[0])?;
            let len = env.eval(&part.size[1])?;
            Capsule3d::new(r, len).mesh().build()
        }
        other => return Err(format!("未知图元类型: {}", other)),
    };

    // 2. 应用旋转（顺序：Z → Y → X，更符合常见欧拉角直觉）
    let mut q = Quat::IDENTITY;
    if let Some(ref e) = part.rotate_z { q = q * Quat::from_rotation_z(env.eval(e)?); }
    if let Some(ref e) = part.rotate_y { q = q * Quat::from_rotation_y(env.eval(e)?); }
    if let Some(ref e) = part.rotate_x { q = q * Quat::from_rotation_x(env.eval(e)?); }
    let mesh = mesh.rotated_by(q);

    // 3. 应用平移
    let mut tr = Vec3::ZERO;
    if part.translate.len() == 3 {
        tr.x = env.eval(&part.translate[0])?;
        tr.y = env.eval(&part.translate[1])?;
        tr.z = env.eval(&part.translate[2])?;
    } else if !part.translate.is_empty() {
        return Err(format!("translate 需要 0 或 3 个元素，got {}", part.translate.len()));
    }
    Ok(mesh.translated_by(tr))
}

/// 颜色字符串 → 线性 [f32;4]（hex→srgb→linear 粗略转换，够用）
pub fn color_from_str(s: &str) -> Option<[f32; 4]> {
    let hex = s.trim().trim_start_matches('#').trim_start_matches("0x");
    if hex.len() != 6 && hex.len() != 8 { return None; }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()? as f32 / 255.0;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()? as f32 / 255.0;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()? as f32 / 255.0;
    let a = if hex.len() == 8 {
        u8::from_str_radix(&hex[6..8], 16).ok()? as f32 / 255.0
    } else { 1.0 };
    // sRGB → 线性
    let conv = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    Some([conv(r), conv(g), conv(b), a])
}

#[test]
fn test_expr_eval() {
    let constants = HashMap::from([("CH".into(), 0.55)]);
    let anchors = HashMap::from([
        ("A_top".into(), Anchor2 { y: Expr::Str("CH * 2.00".into()), z: Expr::Num(0.0) }),
    ]);
    let mut env = ExprEnv::new(&constants, &anchors);
    let v = env.eval(&Expr::Str("A_top.y + 1".into())).unwrap();
    assert!((v - (0.55 * 2.00 + 1.0)).abs() < 1e-5, "v={}", v);
    let v2 = env.eval(&Expr::Str("sqrt(4)".into())).unwrap();
    assert!((v2 - 2.0).abs() < 1e-5);
    let v3 = env.eval(&Expr::Str("atan2(1, 1)".into())).unwrap();
    assert!((v3 - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
}
