//! 从 gen-model `src/fast_model/manifold_csg.rs` 移植的转换（ADR-029 同口径），
//! 入参改成切片，不沾 PlantMesh / 数据库。与浏览器端
//! `web/public/virtual-hole/boolean.js` 逐行对齐——改一处要改另一处。
//!
//! 两处与 gen-model 原版的有意偏差：
//! - 退化三角形判据统一在 f64 上算（f32 顶点在 f64 里精确），保证与 JS 端逐位一致；
//! - 预览网格不需要写盘，光顺法线（10° 组）降为调用方的平面法线。

use glam::{DMat4, DVec3};
use manifold_csg::Manifold;

use crate::{drop_degenerate_tris, weld_mesh_f64, MeshData};

/// glam 列主序 4×4 → manifold-csg 的 4×3 仿射（列主序，末列平移）。
pub fn dmat4_to_affine4x3(m: DMat4) -> [f64; 12] {
    [
        m.x_axis.x, m.x_axis.y, m.x_axis.z, m.y_axis.x, m.y_axis.y, m.y_axis.z, m.z_axis.x,
        m.z_axis.y, m.z_axis.z, m.w_axis.x, m.w_axis.y, m.w_axis.z,
    ]
}

/// 三角网格（f32 顶点，局部毫米）→ Manifold 实体。先按 `mat` 变换再按精确坐标焊接
/// （位模式，`-0.0` 归一，无位置容差），然后 `from_mesh_f64`。
/// 非流形 / 开口网格由 Manifold 内核判错，原样上浮。
pub fn mesh_to_manifold(
    positions: &[[f32; 3]],
    indices: &[u32],
    mat: DMat4,
) -> Result<Manifold, String> {
    if indices.len() < 3 || positions.len() < 3 {
        return Err(format!(
            "mesh has no triangles (verts={} idx={})",
            positions.len(),
            indices.len()
        ));
    }
    if indices.iter().any(|&i| i as usize >= positions.len()) {
        return Err("mesh index out of bounds".into());
    }
    // 先把顶点变换到 f64，再焊接——焊接键落在变换后的坐标上。
    let transformed: Vec<[f64; 3]> = positions
        .iter()
        .map(|v| {
            let p = mat.transform_point3(DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64));
            [p.x, p.y, p.z]
        })
        .collect();
    let (vert_props, tri) = weld_mesh_f64(&transformed, indices);
    if tri.iter().any(|&i| i == u64::MAX) {
        return Err("mesh index out of bounds".into());
    }
    Manifold::from_mesh_f64(&vert_props, 3, &tri)
        .map_err(|error| format!("manifold-csg ingest failed: {error}"))
}

/// Manifold 实体 → 渲染网格（f32 顶点 + u32 索引，退化三角形已丢）。
/// 结果为空（无三角形）时返回 `None`——调用方按 Failed 处理，不覆盖原网格。
pub fn manifold_to_mesh(solid: &Manifold) -> Option<MeshData> {
    let (props, n_props, tri) = solid.to_mesh_f64();
    if n_props < 3 || tri.len() < 3 || props.len() < n_props {
        return None;
    }
    let vert_count = props.len() / n_props;
    let mut positions = Vec::with_capacity(vert_count);
    for i in 0..vert_count {
        let base = i * n_props;
        positions.push([
            props[base] as f32,
            props[base + 1] as f32,
            props[base + 2] as f32,
        ]);
    }
    let mut indices64 = Vec::with_capacity(tri.len());
    for face in tri.chunks_exact(3) {
        if face.iter().any(|&i| i as usize >= vert_count) {
            continue;
        }
        indices64.extend_from_slice(face);
    }
    let indices32: Vec<u32> = indices64.iter().map(|&i| i as u32).collect();
    let indices = drop_degenerate_tris(&positions, &indices32);
    if indices.len() < 3 {
        return None;
    }
    Some(MeshData { positions, indices })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_mesh(size: f32) -> MeshData {
        let p: Vec<[f32; 3]> = (0..8)
            .map(|i| {
                [
                    if i & 1 == 0 { 0.0 } else { size },
                    if i & 2 == 0 { 0.0 } else { size },
                    if i & 4 == 0 { 0.0 } else { size },
                ]
            })
            .collect();
        let faces = [
            [0, 2, 3, 0, 3, 1],
            [4, 5, 7, 4, 7, 6],
            [0, 1, 5, 0, 5, 4],
            [2, 6, 7, 2, 7, 3],
            [1, 3, 7, 1, 7, 5],
            [0, 4, 6, 0, 6, 2],
        ];
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for f in faces {
            let base = positions.len() as u32;
            for &v in &f {
                positions.push(p[v]);
            }
            indices.extend([base, base + 1, base + 2, base + 3, base + 4, base + 5]);
        }
        MeshData { positions, indices }
    }

    #[test]
    fn welded_cube_ingests_and_round_trips() {
        let mesh = cube_mesh(100.0);
        let solid = mesh_to_manifold(&mesh.positions, &mesh.indices, DMat4::IDENTITY).unwrap();
        assert!((solid.volume() - 1_000_000.0).abs() < 1.0);
        let back = manifold_to_mesh(&solid).unwrap();
        assert_eq!(back.tri_count(), 12);
    }

    #[test]
    fn open_mesh_is_rejected() {
        // 缺一个面的箱：非流形。
        let mut mesh = cube_mesh(10.0);
        mesh.indices.truncate(mesh.indices.len() - 6);
        assert!(mesh_to_manifold(&mesh.positions, &mesh.indices, DMat4::IDENTITY).is_err());
    }

    #[test]
    fn empty_mesh_is_rejected() {
        assert!(mesh_to_manifold(&[], &[], DMat4::IDENTITY).is_err());
    }
}
