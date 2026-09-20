//! 房间查询的真库探针：对着本地 SurrealDB 把三个入口各跑一遍，验证
//! 查询语句在真实引擎上可执行、反序列化形状对得上。断言只钉「不炸 +
//! 数据自洽」，不钉具体数量——那取决于库里的项目数据。
//!
//! 前置条件：工作目录（workspace 根）有指向本地库的 `DbOption.toml`，
//! 且该项目跑过 gen-model 的房间构建（`room_relate` 非空才有验证价值）。

use plant_ui_data::room;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "需要本地 SurrealDB（DbOption.toml），用 --ignored 显式跑"]
async fn probe_room_queries_round_trip() {
    // cargo 给集成测试的 CWD 是 crate 目录，而 DbOption.toml 在 workspace 根。
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
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
