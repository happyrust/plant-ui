use plant_ui_data::RefU64;

#[tokio::test]
#[ignore = "live AMS 8009 diagnostic"]
async fn csv7801_models_and_meshes_are_displayable() {
    std::env::set_current_dir(r"D:\work\plant-code\old\test-worklspace\bin").unwrap();
    let mut db = aios_core::options::DbOption::default();
    db.v_ip = "ws://127.0.0.1".into();
    db.v_port = 8009;
    db.v_user = "root".into();
    db.v_password = "root".into();
    db.surreal_ns = "1516".into();
    db.project_name = "AvevaMarineSample".into();
    db.mdb_name = "ALL".into();
    aios_core::set_db_option(db).unwrap();
    plant_ui_data::connect().await.unwrap();

    let root = RefU64::from(((24384_u64) << 32) | 24935);
    let models = plant_ui_data::model_instances(&[root]).await.unwrap();
    assert!(!models.is_empty());
    let mesh_dir = std::path::Path::new(
        r"D:\work\plant-code\old\test-worklspace\bin\assets\meshes",
    );
    for model in &models {
        eprintln!("model={:?} owner={:?} aabb={:?} insts={}", model.refno, model.owner, model.world_aabb, model.insts.len());
        for inst in &model.insts {
            let path = mesh_dir.join(format!("{}.mesh", inst.geo_hash));
            let mesh = aios_core::shape::pdms_shape::PlantMesh::des_mesh_file(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            eprintln!("  hash={} vertices={} triangles={}", inst.geo_hash, mesh.vertices.len(), mesh.indices.len() / 3);
        }
    }
}
