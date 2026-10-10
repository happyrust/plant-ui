//! `plant-room`：房间体判定（plant-ui ADR-0029）。
//!
//! 「一个点在不在房间里」这个问题，gen-model 算房间归属时要问，plant-ui 截断直段时也要问，
//! 两边只能有一份答案。两份实现一旦漂移，同一个点会在归属里算内、在截断里算外，而且不会报错。
//!
//! - [`predicate`]：搬自 gen-model `src/fast_model/room_predicate.rs`，即 2026-09-18 起的三射线奇偶
//!   判定（面板网格先焊接、剔退化三角）。只改了一处：[`panel_tri_mesh`] 的入参从 `&PlantMesh`
//!   换成顶点 / 索引切片，本库因此不依赖 aios_core。
//! - [`solid`]：新写的。一间房全部在册面板合成的房间体，以及「一根线段有哪几截落在房间体内」。
//!
//! # 依赖方向
//! 只依赖 glam 0.29 与 parry3d 0.17：不读库、不走 HTTP，wasm 编得动。
//!
//! # 现状
//! 暂时放在 plant-ui 工作区（`crates/plant-room`）。计划 M0 的后一半还没做：让 gen-model 改用本库，
//! 它的 `room_predicate.rs` 只留 `PlantMesh` 适配。这一步要求零行为变化，要走 gen-model 自己的评审。

pub mod predicate;
pub mod solid;

#[cfg(test)]
pub(crate) mod fixtures;

pub use predicate::{
    AabbVerdict, aabb_is_usable, any_point_inside, center_distance, classify_by_aabb,
    count_vertices_inside, element_in_panel, membership_by_aabb, panel_tri_mesh,
    point_inside_panel, verdict_of,
};
pub use solid::{MIN_PIECE_MM, RoomSolid};
/// 本库签名里的 `TriMesh` / `Aabb` 都来自这一份 parry3d。宿主经这里取用，版本才不会与本库错开。
pub use parry3d;
