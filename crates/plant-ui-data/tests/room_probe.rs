//! 房间查询的真库探针：对着本地 SurrealDB 把三个入口各跑一遍，验证
//! 查询语句在真实引擎上可执行、反序列化形状对得上。断言只钉「不炸 +
//! 数据自洽」，不钉具体数量——那取决于库里的项目数据。
//!
//! 前置条件：工作目录（workspace 根）有指向本地库的 `DbOption.toml`，
//! 且该项目跑过 gen-model 的房间构建（`room_relate` 非空才有验证价值）。
//!
//! 库里那台不是 8009（gen-model 的 rocksdb 落在别的端口）时，给
//! `PLANT_TEST_DB_PORT`（与 `mesh_compat.rs` 同一格）压过 `DbOption.toml`。
//!
//! 「成员数」这条断言是 `room_detail` 的回归网：`room_relate` 上按数组取 `in`
//! 曾静默返回 0 行，成员数整间变 0、截断候选跟着空（2026-10-25 修复，见
//! `room.rs::room_detail_sql`）。总览走全表扫不受影响，两边一比就现形。

use plant_ui_data::room;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "需要本地 SurrealDB（DbOption.toml），用 --ignored 显式跑"]
async fn probe_room_queries_round_trip() {
    // cargo 给集成测试的 CWD 是 crate 目录，而 DbOption.toml 在 workspace 根。
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
    // 指到别的库（如 gen-model 的 rocksdb 端口）时显式造一份配置，压过 DbOption.toml。
    if let Ok(port) = std::env::var("PLANT_TEST_DB_PORT") {
        let port: u16 = port.parse().expect("PLANT_TEST_DB_PORT 不是有效端口");
        let mut db = aios_core::options::DbOption::default();
        db.v_ip = "ws://127.0.0.1".into();
        db.v_port = port;
        db.v_user = "root".into();
        db.v_password = "root".into();
        db.surreal_ns = std::env::var("PLANT_TEST_DB_NS").unwrap_or_else(|_| "1516".into());
        db.project_name =
            std::env::var("PLANT_TEST_DB_NAME").unwrap_or_else(|_| "AvevaMarineSample".into());
        db.mdb_name = std::env::var("PLANT_TEST_MDB").unwrap_or_else(|_| "ALL".into());
        aios_core::set_db_option(db).expect("数据库配置设置失败");
    }
    plant_ui_data::connect().await.expect("连接本地库失败");

    let overview = room::rooms_overview().await.expect("rooms_overview 失败");
    println!(
        "rooms_overview: {} 间（合规 {} / 不合规 {}），待重算 {}",
        overview.len(),
        overview.iter().filter(|r| r.valid_name).count(),
        overview.iter().filter(|r| !r.valid_name).count(),
        overview.iter().filter(|r| r.pending_recalc).count(),
    );

    // 找一间有成员的合规房间，把 detail 与 element_rooms 串起来验证一致性。
    let Some(target) = overview.iter().find(|r| r.valid_name && r.member_count > 0) else {
        println!("库里没有带成员的合规房间，探针只验证了 overview 这一层");
        return;
    };

    let detail = room::room_detail(target.room.into(), 5)
        .await
        .expect("room_detail 失败")
        .expect("overview 里的房间在 detail 侧查不到");
    println!(
        "room_detail {}: 面板 {} / 成员 {} / 预览 {} / 待重算 {}",
        detail.room_num,
        detail.panels.len(),
        detail.member_count,
        detail.members.len(),
        detail.pending_recalc,
    );
    assert_eq!(detail.room_num, target.room_num, "两条路径的房号不一致");
    assert_eq!(
        detail.member_count, target.member_count,
        "detail 与 overview 的去重成员数不一致"
    );
    assert_eq!(
        detail.member_refnos.len(),
        detail.member_count,
        "全量成员清单与去重计数不一致"
    );
    assert!(detail.members.len() <= 5, "预览超出上限");

    // 直段截断计划 M5：点名的几间房逐间量 `room_detail`（含截断候选那一趟成员直选）的耗时，
    // 记下候选数。`PLANT_ROOM_PROBE_ROOMS=R320,R731` 这样给房号；不给就跳过。
    if let Ok(names) = std::env::var("PLANT_ROOM_PROBE_ROOMS") {
        for room_num in names.split(',').map(str::trim).filter(|name| !name.is_empty()) {
            let Some(row) = overview.iter().find(|r| r.room_num == room_num) else {
                println!("room_detail {room_num}: 总览里没有这间房");
                continue;
            };
            let started = std::time::Instant::now();
            let detail = room::room_detail(row.room.into(), 5)
                .await
                .expect("room_detail 失败")
                .expect("总览里的房间在 detail 侧查不到");
            let elapsed = started.elapsed();
            println!(
                "room_detail {room_num}: 面板 {} / 成员 {} / 截断候选 {} / {:.0} ms",
                detail.panels.len(),
                detail.member_count,
                detail.tube_carriers.len(),
                elapsed.as_secs_f64() * 1000.0,
            );
            // 候选按成员强度序去重，一个都不许重复。
            let unique: std::collections::HashSet<_> = detail.tube_carriers.iter().collect();
            assert_eq!(unique.len(), detail.tube_carriers.len(), "{room_num} 的截断候选有重复");
        }
    }

    // 拿预览里最强的成员反查它的房间，必须能查到这间房，且排序键一致。
    let Some(member) = detail.members.first() else {
        return;
    };
    let relations = room::element_rooms(member.refno.into())
        .await
        .expect("element_rooms 失败");
    println!(
        "element_rooms {}: {} 条归属，最强 {:?}",
        member.refno.to_pdms_str(),
        relations.len(),
        relations.first().map(|r| (&r.room_num, r.inside_count)),
    );
    assert!(
        relations.iter().any(|r| r.room_num == detail.room_num),
        "成员 {} 反查不到它所在的房间 {}",
        member.refno.to_pdms_str(),
        detail.room_num,
    );
    // 返回序必须是强度全序：inside 降、dist 升。
    for pair in relations.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        assert!(
            a.inside_count > b.inside_count
                || (a.inside_count == b.inside_count && a.center_dist <= b.center_dist),
            "归属排序违反 ADR-010 §5：{:?} 排在 {:?} 前",
            (&a.room_num, a.inside_count, a.center_dist),
            (&b.room_num, b.inside_count, b.center_dist),
        );
    }
}
