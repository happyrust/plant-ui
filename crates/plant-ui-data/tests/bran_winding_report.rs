//! 临时探针：定位 BRAN 24381_145018 里绕序与法线相反的三角形分布。拿到结论即删。

use aios_core::shape::pdms_shape::PlantMesh;
use std::collections::BTreeMap;

#[tokio::test]
async fn report_opposed_triangles() {
    std::env::set_current_dir(r"D:\work\plant-code\old\plant-ui").unwrap();
    plant_ui_data::connect().await.unwrap();
    let root = "24381_145018".into();
    let refnos = aios_core::query_deep_visible_inst_refnos(root)
        .await
        .unwrap();
    let models = aios_core::query_insts(refnos.iter(), true).await.unwrap();

    let mesh_dir = std::path::Path::new(r"D:\work\plant-code\old\gen-model\assets\meshes");
    // geo_hash -> (总三角数, 反向数, 反向里 |dot| 相对量的最大值, 所属元素)
    let mut per_geo: BTreeMap<String, (usize, usize, f32, Vec<String>)> = BTreeMap::new();

    for model in &models {
        for inst in &model.insts {
            let mesh = PlantMesh::des_mesh_file(&mesh_dir.join(format!("{}.mesh", inst.geo_hash)))
                .unwrap();
            let entry = per_geo
                .entry(inst.geo_hash.clone())
                .or_insert((0, 0, 0.0, Vec::new()));
            let owner_tag = format!("{}", model.refno);
            if !entry.3.contains(&owner_tag) {
                entry.3.push(owner_tag);
            }
            for triangle in mesh.indices.chunks_exact(3) {
                let [a, b, c] = [
                    triangle[0] as usize,
                    triangle[1] as usize,
                    triangle[2] as usize,
                ];
                let face = (mesh.vertices[b] - mesh.vertices[a])
                    .cross(mesh.vertices[c] - mesh.vertices[a]);
                let normal = mesh.normals[a] + mesh.normals[b] + mesh.normals[c];
                entry.0 += 1;
                let dot = face.dot(normal);
                if dot < 0.0 {
                    entry.1 += 1;
                    let denom = face.length() * normal.length();
                    let rel = if denom > 0.0 { -dot / denom } else { 0.0 };
                    if rel > entry.2 {
                        entry.2 = rel;
                    }
                }
            }
        }
    }

    println!("geo_hash | tris | opposed | worst(|cos|) | elements");
    for (hash, (tris, opposed, worst, owners)) in &per_geo {
        if *opposed > 0 {
            println!(
                "{hash} | {tris} | {opposed} | {worst:.4} | {}",
                owners.join(",")
            );
        }
    }
    let total_opposed: usize = per_geo.values().map(|v| v.1).sum();
    println!("total opposed: {total_opposed}");
}
