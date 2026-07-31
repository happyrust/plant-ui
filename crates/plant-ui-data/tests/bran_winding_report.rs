//! 临时探针：定位 BRAN 24381_145018 里绕序与法线相反的三角形分布。拿到结论即删。

use aios_core::shape::pdms_shape::PlantMesh;
use std::collections::BTreeMap;

/// 前置条件：本机 SurrealDB 在跑（连哪台取工作区根的 `DbOption.toml`），且
/// `PLANT_TEST_MESH_DIR` 指向 gen-model 产出的 meshes 目录。
#[tokio::test]
#[ignore = "需要本机 SurrealDB 与 PLANT_TEST_MESH_DIR，用 --ignored 显式跑"]
async fn report_opposed_triangles() {
    // rs-core 的 `try_get_db_option` 拿 `File::with_name("DbOption")` 按进程 CWD 找配置，
    // 所以连库前得先站到工作区根上——DbOption.toml 在那里。
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .expect("无法切换到工作区目录");
    plant_ui_data::connect().await.unwrap();
    let root = "24381_145018".into();
    let refnos = aios_core::query_deep_visible_inst_refnos(root)
        .await
        .unwrap();
    let models = aios_core::query_insts(refnos.iter(), true).await.unwrap();

    let mesh_dir = std::env::var_os("PLANT_TEST_MESH_DIR").expect("PLANT_TEST_MESH_DIR 未设置");
    let mesh_dir = std::path::Path::new(&mesh_dir);
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
