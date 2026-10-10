//! 测试用的面板网格。

use glam::Vec3;

/// 一个闭合的盒子，索引排布与 gen-model `room_model::box_mesh_for_test` 逐位相同：
/// 退化三角那条用例改的是 `indices[30..36]`（+x 面）里的 6 号顶点，排布变了用例就测偏了。
pub fn box_mesh(min: Vec3, max: Vec3) -> (Vec<Vec3>, Vec<u32>) {
    let vertices = vec![
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ];
    #[rustfmt::skip]
    let indices = vec![
        0, 2, 1,  0, 3, 2, // -z
        4, 5, 6,  4, 6, 7, // +z
        0, 1, 5,  0, 5, 4, // -y
        3, 7, 6,  3, 6, 2, // +y
        0, 4, 7,  0, 7, 3, // -x
        1, 2, 6,  1, 6, 5, // +x
    ];
    (vertices, indices)
}

/// 把一个简单多边形沿 z 拉成闭合棱柱，和房间面板一样是「盖面 + 侧壁」。
///
/// 盖面按顶点 0 扇形三角化，所以调用方得保证从顶点 0 看得见每一条边（测试里的 L 形满足）。
/// 奇偶判定不看绕序，这里也就不管朝向。
pub fn prism(polygon: &[[f32; 2]], z0: f32, z1: f32) -> (Vec<Vec3>, Vec<u32>) {
    let n = polygon.len() as u32;
    let mut vertices: Vec<Vec3> = polygon.iter().map(|[x, y]| Vec3::new(*x, *y, z0)).collect();
    vertices.extend(polygon.iter().map(|[x, y]| Vec3::new(*x, *y, z1)));
    let mut indices = Vec::new();
    for i in 1..n - 1 {
        indices.extend([0, i + 1, i]);
        indices.extend([n, n + i, n + i + 1]);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        indices.extend([i, j, n + j]);
        indices.extend([i, n + j, n + i]);
    }
    (vertices, indices)
}
