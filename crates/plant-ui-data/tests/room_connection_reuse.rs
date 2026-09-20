//! Real mirror regression: concurrent selection requests must share one connection.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PLANT_ROOM_TEST_DIR pointing to an isolated DbOption.toml directory"]
async fn concurrent_room_queries_reuse_the_connection() {
    let dir = std::env::var("PLANT_ROOM_TEST_DIR").expect("isolated configuration required");
    std::env::set_current_dir(dir).unwrap();
    let results = futures::future::join_all((0..8).map(|_| plant_ui_data::connect())).await;
    for result in results { result.expect("concurrent connect"); }
    plant_ui_data::connect().await.expect("repeated connect");
    let refno = "24384/26599".parse::<plant_ui_data::RefU64>().unwrap().into();
    plant_ui_data::room::element_rooms(refno).await.expect("element rooms");
    plant_ui_data::room::panel_room(refno).await.expect("panel room");
}
