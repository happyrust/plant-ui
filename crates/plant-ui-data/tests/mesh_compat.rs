use aios_core::shape::pdms_shape::PlantMesh;

/// 前置条件：`PLANT_TEST_MESH_DIR` 指向 gen-model 产出的 meshes 目录。
#[test]
#[ignore = "需要 PLANT_TEST_MESH_DIR，用 --ignored 显式跑"]
fn meshes_from_configured_directory_load() {
    let dir = std::env::var_os("PLANT_TEST_MESH_DIR").expect("PLANT_TEST_MESH_DIR 未设置");
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).expect("mesh 目录无法读取") {
        let path = entry.expect("mesh 目录项无法读取").path();
        if path.extension().is_some_and(|ext| ext == "mesh") {
            let bytes =
                std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let mesh = PlantMesh::des_from_bytes(&bytes)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(
                mesh.vertices.len(),
                mesh.normals.len(),
                "{} 的顶点/法线数量不一致",
                path.display()
            );
            assert_eq!(
                mesh.indices.len() % 3,
                0,
                "{} 的索引不是三角形",
                path.display()
            );
            assert!(
                mesh.indices
                    .iter()
                    .all(|&index| index < mesh.vertices.len() as u32),
                "{} 有越界索引",
                path.display()
            );
            mesh.gen_bevy_mesh();
            count += 1;
        }
    }
    assert!(
        count > 0,
        "{} 中没有 .mesh 文件",
        std::path::Path::new(&dir).display()
    );
    eprintln!("已验证 {count} 个 mesh");
}

/// 前置条件：本机 SurrealDB 在跑，且 `PLANT_TEST_MESH_DIR`、`PLANT_TEST_DB_PORT`、
/// `PLANT_TEST_SITE_NAME` 三个环境变量都已设好。
#[tokio::test]
#[ignore = "需要本机 SurrealDB 与 PLANT_TEST_* 环境变量，用 --ignored 显式跑"]
async fn configured_site_models_have_meshes() {
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .expect("无法切换到工作区目录");
    let dir = std::env::var_os("PLANT_TEST_MESH_DIR").expect("PLANT_TEST_MESH_DIR 未设置");
    let port = std::env::var("PLANT_TEST_DB_PORT")
        .expect("PLANT_TEST_DB_PORT 未设置")
        .parse()
        .expect("PLANT_TEST_DB_PORT 不是有效端口");
    let site_name = std::env::var("PLANT_TEST_SITE_NAME").expect("PLANT_TEST_SITE_NAME 未设置");
    let mut db = aios_core::options::DbOption::default();
    db.v_ip = "ws://127.0.0.1".into();
    db.v_port = port;
    db.v_user = "root".into();
    db.v_password = "root".into();
    db.surreal_ns = "1516".into();
    db.project_name = "AvevaMarineSample".into();
    db.mdb_name = "ALL".into();
    aios_core::set_db_option(db).expect("数据库配置设置失败");

    plant_ui_data::connect().await.expect("数据库连接失败");
    let site = plant_ui_data::site_nodes()
        .await
        .expect("SITE 查询失败")
        .into_iter()
        .find(|site| site.name == site_name)
        .unwrap_or_else(|| panic!("找不到 SITE {site_name}"));
    let models = plant_ui_data::model_instances(&[site.refno.refno()])
        .await
        .expect("模型实例查询失败");
    assert!(!models.is_empty(), "{site_name} 没有模型实例");

    let missing = models
        .iter()
        .flat_map(|model| &model.insts)
        .filter_map(|inst| {
            (!std::path::Path::new(&dir)
                .join(format!("{}.mesh", inst.geo_hash))
                .is_file())
            .then(|| inst.geo_hash.clone())
        })
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "{site_name} 有 {} 个 mesh 文件缺失，示例：{:?}",
        missing.len(),
        &missing[..missing.len().min(10)]
    );
    eprintln!(
        "{site_name}: {} 个元素，{} 个网格实例",
        models.len(),
        models.iter().map(|model| model.insts.len()).sum::<usize>()
    );
}
