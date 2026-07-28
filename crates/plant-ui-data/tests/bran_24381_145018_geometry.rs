use aios_core::shape::pdms_shape::PlantMesh;
use glam::Vec3;

#[tokio::test]
async fn generated_mesh_bounds_match_database_bounds() {
    std::env::set_current_dir(r"D:\work\plant-code\old\plant-ui").unwrap();
    plant_ui_data::connect().await.unwrap();
    let root = "24381_145018".into();
    let refnos = aios_core::query_deep_visible_inst_refnos(root)
        .await
        .unwrap();
    let models = aios_core::query_insts(refnos.iter(), true).await.unwrap();
    assert!(!models.is_empty(), "BRAN 没有生成任何可显示实例");

    let mesh_dir = std::path::Path::new(r"D:\work\plant-code\old\gen-model\assets\meshes");
    let mut stored_min = Vec3::splat(f32::INFINITY);
    let mut stored_max = Vec3::splat(f32::NEG_INFINITY);
    let mut actual_min = Vec3::splat(f32::INFINITY);
    let mut actual_max = Vec3::splat(f32::NEG_INFINITY);
    let mut mesh_count = 0usize;
    let (mut winding_agrees, mut winding_opposes) = (0usize, 0usize);

    for model in &models {
        stored_min = stored_min.min(model.world_aabb.mins.into());
        stored_max = stored_max.max(model.world_aabb.maxs.into());
        for inst in &model.insts {
            let mesh = PlantMesh::des_mesh_file(&mesh_dir.join(format!("{}.mesh", inst.geo_hash)))
                .unwrap();
            let transform = (model.world_trans * inst.transform).compute_matrix();
            for &vertex in &mesh.vertices {
                let point = transform.transform_point3(vertex);
                actual_min = actual_min.min(point);
                actual_max = actual_max.max(point);
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
                if face.dot(normal) >= 0.0 {
                    winding_agrees += 1;
                } else {
                    winding_opposes += 1;
                }
            }
            mesh_count += 1;
        }
    }

    let extent = (stored_max - stored_min).length().max(1.0);
    let error = (stored_min - actual_min)
        .abs()
        .max((stored_max - actual_max).abs())
        .max_element();
    println!(
        "models={:?} meshes={} winding={winding_agrees}/{winding_opposes} stored={stored_min:?}..{stored_max:?} actual={actual_min:?}..{actual_max:?} relative_error={}",
        models
            .iter()
            .map(|model| (model.refno, model.owner))
            .collect::<Vec<_>>(),
        mesh_count,
        error / extent
    );
    assert!(
        error / extent < 1.0e-3,
        "显示几何与数据库世界包围盒不一致，relative_error={}",
        error / extent
    );
    assert_eq!(winding_opposes, 0, "三角形绕序与顶点法线相反");
}
