//! 临时诊断：真实 .mesh 资产的三角形绕序 vs bevy_picking 的背面剔除。
//! 结论拿到后即删。

use aios_core::shape::pdms_shape::PlantMesh;
use glam::Vec3;

/// 与 bevy_picking `ray_triangle_intersection` 同款 Möller–Trumbore。
/// `cull=true` 时 determinant < EPSILON 即拒绝（背面剔除）。
fn ray_tri(origin: Vec3, dir: Vec3, tri: &[Vec3; 3], cull: bool) -> Option<f32> {
    let v01 = tri[1] - tri[0];
    let v02 = tri[2] - tri[0];
    let p = dir.cross(v02);
    let det = v01.dot(p);
    if cull {
        if det < f32::EPSILON {
            return None;
        }
    } else if det.abs() < f32::EPSILON {
        return None;
    }
    let inv = 1.0 / det;
    let t_vec = origin - tri[0];
    let u = t_vec.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = t_vec.cross(v01);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = v02.dot(q) * inv;
    (t >= 0.0).then_some(t)
}

fn ray_mesh(origin: Vec3, dir: Vec3, mesh: &PlantMesh, cull: bool) -> Option<f32> {
    let mut best: Option<f32> = None;
    for tri in mesh.indices.chunks_exact(3) {
        let t = [
            mesh.vertices[tri[0] as usize],
            mesh.vertices[tri[1] as usize],
            mesh.vertices[tri[2] as usize],
        ];
        if let Some(d) = ray_tri(origin, dir, &t, cull) {
            best = Some(best.map_or(d, |b: f32| b.min(d)));
        }
    }
    best
}

#[test]
fn probe_real_mesh_winding() {
    let dir = std::path::Path::new(r"D:\work\plant-code\old\gen-model\assets\meshes");
    let mut checked = 0usize;
    let mut cull_hits = 0usize;
    let mut include_hits = 0usize;
    let mut agree = 0usize;
    let mut oppose = 0usize;
    let mut degenerate = 0usize;

    for entry in std::fs::read_dir(dir).expect("meshes 目录不存在") {
        if checked >= 60 {
            break;
        }
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("mesh") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mesh) = PlantMesh::des_from_bytes(&bytes) else {
            continue;
        };
        if mesh.indices.len() < 3 || mesh.vertices.is_empty() {
            continue;
        }

        // 三角形几何法线（按绕序）与顶点法线的一致性。
        for tri in mesh.indices.chunks_exact(3).take(80) {
            let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
            let geo =
                (mesh.vertices[b] - mesh.vertices[a]).cross(mesh.vertices[c] - mesh.vertices[a]);
            let n = mesh.normals[a] + mesh.normals[b] + mesh.normals[c];
            if geo.length_squared() < 1e-12 || n.length_squared() < 1e-12 {
                degenerate += 1;
                continue;
            }
            if geo.dot(n) > 0.0 {
                agree += 1;
            } else {
                oppose += 1;
            }
        }

        // 从包围盒外沿 -X 方向朝中心打一条必穿过的射线。
        let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for v in &mesh.vertices {
            min = min.min(*v);
            max = max.max(*v);
        }
        let center = (min + max) * 0.5;
        let extent = (max - min).length().max(1.0);
        let origin = center + Vec3::X * extent * 3.0;
        let dir_vec = Vec3::NEG_X;
        if ray_mesh(origin, dir_vec, &mesh, true).is_some() {
            cull_hits += 1;
        }
        if ray_mesh(origin, dir_vec, &mesh, false).is_some() {
            include_hits += 1;
        }
        checked += 1;
    }

    panic!(
        "诊断结果：checked={checked} cull_hits={cull_hits} include_hits={include_hits} \
         绕序vs法线 agree={agree} oppose={oppose} degenerate={degenerate}"
    );
}
