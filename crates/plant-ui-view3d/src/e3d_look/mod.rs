//! E3D 外观（AVEVA E3D 3.1 SGL DX11 口径的显示效果），从 plant3d-web `src/viewer/e3dLook/` 移植。
//! 计划：`docs/plans/2026-10-22-e3d-look-port-plannotator.md`。
//!
//! - [`colour_table`]：PDMS / E3D 3.1 颜色表与出厂图形颜色；
//! - [`presets`]：光照常量、两套预设、背景渐变；
//! - [`material`]：`SglLookMaterial`（前向头灯着色 + 环境反射，prepass 写参与标记）；
//! - [`env_cube`]：sglDx11 内嵌的环境立方体贴图；
//! - `demo`（debug 构建，或开 `look-demo` feature）：`--look-demo`（桌面）/ `?look_demo=1`（浏览器）的演示厂，
//!   不要后端也能做像素验收。
//!
//! - [`post`]：边线（HLR）与伪阴影（HBAO）的后处理链（阶段二）。
//!
//! 模型材质怎么按外观挑在 `crate::look_materials`（两种外观共用）。

pub mod colour_table;
#[cfg(any(debug_assertions, feature = "look-demo"))]
pub mod demo;
pub mod env_cube;
pub mod material;
pub mod post;
pub mod presets;
