use aios_core::shape::pdms_shape::PlantMesh;
use glam::Vec3;

fn aabb_gap(a_min: Vec3, a_max: Vec3, b_min: Vec3, b_max: Vec3) -> f32 {
    (a_min - b_max).max(b_min - a_max).max(Vec3::ZERO).length()
}

/// 前置条件：本机 SurrealDB 在跑（连哪台取工作区根的 `DbOption.toml`），且
/// `PLANT_TEST_MESH_DIR` 指向 gen-model 产出的 meshes 目录。
#[tokio::test]
#[ignore = "需要本机 SurrealDB 与 PLANT_TEST_MESH_DIR，用 --ignored 显式跑"]
async fn generated_branch_is_one_connected_route() {
    // rs-core 的 `try_get_db_option` 拿 `File::with_name("DbOption")` 按进程 CWD 找配置，
    // 所以连库前得先站到工作区根上——DbOption.toml 在那里。
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .expect("无法切换到工作区目录");
    plant_ui_data::connect().await.unwrap();
    let root = "24381_145018".parse::<plant_ui_data::RefU64>().unwrap();
    let models = plant_ui_data::model_instances(&[root]).await.unwrap();
    let tubi_count = models
        .iter()
        .filter(|model| model.insts.iter().any(|inst| inst.is_tubi))
        .count();
    assert_eq!(tubi_count, 11, "BRAN 应包含 11 段隐含直管");
    let bounds = models
        .iter()
        .map(|model| {
            (
                model.refno,
                Vec3::from(model.world_aabb.mins),
                Vec3::from(model.world_aabb.maxs),
            )
        })
        .collect::<Vec<_>>();

    assert!(!bounds.is_empty(), "BRAN 没有生成任何可显示实例");
    let mut reached = vec![false; bounds.len()];
    reached[0] = true;
    loop {
        let mut changed = false;
        for i in 0..bounds.len() {
            if !reached[i] {
                continue;
            }
            for j in 0..bounds.len() {
                if !reached[j]
                    && aabb_gap(bounds[i].1, bounds[i].2, bounds[j].1, bounds[j].2) <= 10.0
                {
                    reached[j] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let disconnected = bounds
        .iter()
        .zip(reached)
        .filter_map(|((refno, _, _), reached)| (!reached).then_some(*refno))
        .collect::<Vec<_>>();
    assert!(
        disconnected.is_empty(),
        "BRAN 生成构件没有连成一条管路，断开的参考号：{disconnected:?}"
    );
    assert_generated_mesh_bounds_match_database_bounds().await;
}

async fn assert_generated_mesh_bounds_match_database_bounds() {
    let root = "24381_145018".into();
    let refnos = aios_core::query_deep_visible_inst_refnos(root)
        .await
        .unwrap();
    let models = aios_core::query_insts(refnos.iter(), true).await.unwrap();
    assert!(!models.is_empty(), "BRAN 没有生成任何可显示实例");

    let mesh_dir = std::env::var_os("PLANT_TEST_MESH_DIR").expect("PLANT_TEST_MESH_DIR 未设置");
    let mesh_dir = std::path::Path::new(&mesh_dir);
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
