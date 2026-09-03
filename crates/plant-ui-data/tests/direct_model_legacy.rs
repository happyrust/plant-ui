use aios_core::shape::pdms_shape::PlantMesh;

/// End-to-end contract for Core3D direct generation persisted in gen-model's
/// legacy `inst_relate.booled_id` and `inst_geo + geo_relate.trans`
/// representations.
#[tokio::test]
#[ignore = "requires the isolated direct-model SurrealDB fixture"]
async fn direct_models_follow_the_plant_ui_legacy_load_path() {
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .expect("workspace cwd");
    let mesh_dir = std::path::PathBuf::from(
        std::env::var_os("PLANT_TEST_MESH_DIR").expect("PLANT_TEST_MESH_DIR"),
    );
    let port = std::env::var("PLANT_TEST_DB_PORT")
        .expect("PLANT_TEST_DB_PORT")
        .parse()
        .expect("valid port");
    let expected: usize = std::env::var("PLANT_TEST_EXPECTED_DIRECT")
        .unwrap_or_else(|_| "4476".into())
        .parse()
        .expect("valid expected count");
    let expected_unique: usize = std::env::var("PLANT_TEST_EXPECTED_UNIQUE")
        .unwrap_or_else(|_| expected.to_string())
        .parse()
        .expect("valid expected unique count");
    let expected_shared: usize = std::env::var("PLANT_TEST_EXPECTED_SHARED")
        .unwrap_or_else(|_| "0".into())
        .parse()
        .expect("valid expected shared count");

    let mut option = aios_core::options::DbOption::default();
    option.v_ip = "ws://127.0.0.1".into();
    option.v_port = port;
    option.v_user = "root".into();
    option.v_password = "root".into();
    option.surreal_ns = "1516".into();
    option.project_name = "AvevaMarineSample".into();
    option.mdb_name = "ALL".into();
    aios_core::set_db_option(option).expect("set database fixture");
    plant_ui_data::connect().await.expect("connect fixture");

    let mut response = aios_core::SUL_DB
        .query(
            "SELECT VALUE <string>record::id(in) FROM inst_relate \
             WHERE direct_model.format IN ['legacy-v1', 'legacy-instanced-v2'];",
        )
        .await
        .expect("query direct model refnos")
        .check()
        .expect("valid direct model query");
    let raw_refnos: Vec<String> = response.take(0).expect("decode refnos");
    assert_eq!(raw_refnos.len(), expected, "persisted direct model count");
    let refnos = raw_refnos
        .iter()
        .map(|raw| aios_core::RefnoEnum::from(raw.as_str()))
        .collect::<Vec<_>>();

    let mut models = Vec::with_capacity(expected);
    for chunk in refnos.chunks(250) {
        models.extend(
            aios_core::query_insts(chunk.iter(), true)
                .await
                .expect("Plant UI query_insts"),
        );
    }
    assert_eq!(models.len(), expected, "Plant UI decoded model count");

    let mut unique_meshes = std::collections::BTreeSet::new();
    let mut shared_models = 0usize;
    for model in &models {
        assert_eq!(model.insts.len(), 1, "direct model has one visible mesh");
        shared_models += usize::from(!model.has_neg);
        for instance in &model.insts {
            if !unique_meshes.insert(instance.geo_hash.clone()) {
                continue;
            }
            let path = mesh_dir.join(format!("{}.mesh", instance.geo_hash));
            let mesh = PlantMesh::des_mesh_file(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(
                mesh.vertices.len(),
                mesh.normals.len(),
                "{}",
                path.display()
            );
            assert_eq!(mesh.indices.len() % 3, 0, "{}", path.display());
            assert!(
                mesh.indices
                    .iter()
                    .all(|&index| index < mesh.vertices.len() as u32),
                "{} has an out-of-range index",
                path.display()
            );
            mesh.gen_bevy_mesh();
        }
    }
    assert_eq!(unique_meshes.len(), expected_unique, "unique mesh count");
    assert_eq!(shared_models, expected_shared, "shared instance count");
    eprintln!(
        "PLANT_UI_DIRECT_COMPAT models={} mesh_instances={} unique_meshes={} shared_instances={}",
        models.len(),
        models.iter().map(|model| model.insts.len()).sum::<usize>(),
        unique_meshes.len(),
        shared_models,
    );
}
