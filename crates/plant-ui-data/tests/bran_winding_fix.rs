//! 临时修复器：把两个 .mesh 里绕序与顶点法线相反的三角形翻回来（swap b/c）。
//! 顶点/法线不动，包围盒不变；内容哈希键的是几何参数，不受网格字节影响。
//! 跑完确认测试转绿后即删。

use aios_core::shape::pdms_shape::PlantMesh;

const TARGETS: [&str; 2] = ["10628572723856983635", "14763471500668989479"];

#[test]
fn flip_opposed_triangles_in_place() {
    let mesh_dir = std::path::Path::new(r"D:\work\plant-code\old\gen-model\assets\meshes");
    let backup_dir = std::path::Path::new(r"D:\work\plant-code\old\plant-ui\.context\mesh-backup");
    std::fs::create_dir_all(backup_dir).unwrap();

    for hash in TARGETS {
        let path = mesh_dir.join(format!("{hash}.mesh"));
        let backup = backup_dir.join(format!("{hash}.mesh"));
        if !backup.exists() {
            std::fs::copy(&path, &backup).unwrap();
        }

        let mut mesh = PlantMesh::des_mesh_file(&path).unwrap();
        let mut flipped = 0usize;
        for triangle in mesh.indices.chunks_exact_mut(3) {
            let [a, b, c] = [
                triangle[0] as usize,
                triangle[1] as usize,
                triangle[2] as usize,
            ];
            let face =
                (mesh.vertices[b] - mesh.vertices[a]).cross(mesh.vertices[c] - mesh.vertices[a]);
            let normal = mesh.normals[a] + mesh.normals[b] + mesh.normals[c];
            if face.dot(normal) < 0.0 {
                triangle.swap(1, 2);
                flipped += 1;
            }
        }

        // 复验：翻转后不允许再有反向三角形
        let mut remaining = 0usize;
        for triangle in mesh.indices.chunks_exact(3) {
            let [a, b, c] = [
                triangle[0] as usize,
                triangle[1] as usize,
                triangle[2] as usize,
            ];
            let face =
                (mesh.vertices[b] - mesh.vertices[a]).cross(mesh.vertices[c] - mesh.vertices[a]);
            let normal = mesh.normals[a] + mesh.normals[b] + mesh.normals[c];
            if face.dot(normal) < 0.0 {
                remaining += 1;
            }
        }
        assert_eq!(remaining, 0, "{hash} 翻转后仍有反向三角形");

        mesh.ser_to_file(&path).unwrap();
        println!(
            "{hash}: flipped {flipped} triangles, backup at {}",
            backup.display()
        );
    }
}
