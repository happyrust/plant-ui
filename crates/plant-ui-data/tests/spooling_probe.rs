//! Spooling 查询的真库探针：对着本地 SurrealDB 把三个入口各跑一遍，
//! 验证查询语句在真实引擎上可执行、反序列化形状对得上。
//!
//! 断言只钉「不炸 + 形状自洽」，**不钉数量**——AMS `/ALL` 里
//! `SPLDRG` / `SPOOL` 一个都没有（292 个 DESI 库全扫确认过），
//! 有 spool 的 `ams251095` / `ams251137` 又不在 `/ALL` 的 `CURD` 名单里，
//! 要按 dbnum 点名初始化后才有数据。没初始化时全 0 是**数据现状**。
//!
//! 一条例外：只要库里有 `PSPOOL` / `PPIECE` / `HSPOOL`，它们的成员就**不能为空**——
//! 这三类在 `pe_owner` 下没有子层，成员全靠 noun 表上的引用属性，回空就是
//! 属性那一槛读错了表。
//!
//! 前置条件：工作目录（workspace 根）有指向本地库的 `DbOption.toml`。
//! 库里那台不是 8009 时给 `PLANT_TEST_DB_PORT` 压过（与 `room_probe.rs` 同一格）。

use plant_ui_data::spooling::{self, SPOOL_GROUPS};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "需要本地 SurrealDB（DbOption.toml），用 --ignored 显式跑"]
async fn probe_spooling_queries_round_trip() {
    // cargo 给集成测试的 CWD 是 crate 目录，而 DbOption.toml 在 workspace 根。
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
    if let Ok(port) = std::env::var("PLANT_TEST_DB_PORT") {
        let port: u16 = port.parse().expect("PLANT_TEST_DB_PORT 不是有效端口");
        // 字段在构造时一次给全，不先 Default 再逐字段赋值：那样写少一个字段也编得过，
        // 却会在运行期拿到与 `DbOption.toml` 不一致的默认值。
        let db = aios_core::options::DbOption {
            v_ip: "ws://127.0.0.1".into(),
            v_port: port,
            v_user: "root".into(),
            v_password: "root".into(),
            surreal_ns: std::env::var("PLANT_TEST_DB_NS").unwrap_or_else(|_| "1516".into()),
            project_name: std::env::var("PLANT_TEST_DB_NAME")
                .unwrap_or_else(|_| "AvevaMarineSample".into()),
            mdb_name: std::env::var("PLANT_TEST_MDB").unwrap_or_else(|_| "ALL".into()),
            ..Default::default()
        };
        aios_core::set_db_option(db).expect("数据库配置设置失败");
    }
    plant_ui_data::connect().await.expect("连接本地库失败");

    let overview = spooling::spooling_overview()
        .await
        .expect("spooling_overview 失败");
    assert_eq!(
        overview.len(),
        SPOOL_GROUPS.len(),
        "统计行必须与分组表一一对应，缺一行界面就少一栏"
    );
    for (group, (noun, label)) in overview.iter().zip(SPOOL_GROUPS) {
        assert_eq!(&group.noun, noun);
        assert_eq!(&group.label, label);
    }
    println!(
        "spooling_overview: {}",
        overview
            .iter()
            .map(|g| format!("{}={}", g.noun, g.count))
            .collect::<Vec<_>>()
            .join(" / ")
    );

    // 每一类都查一次列表。全 0 也算通过——那说明查询能跑、只是库里没有。
    for (noun, _) in SPOOL_GROUPS {
        let rows = spooling::spool_rows(noun, 20)
            .await
            .unwrap_or_else(|e| panic!("spool_rows({noun}) 失败: {e}"));
        for row in &rows {
            assert_eq!(&row.noun, noun, "列表里混进了别的 noun");
            assert!(
                !row.owner_noun.is_empty(),
                "{} 的属主 noun 没查出来",
                row.refno
            );
        }
        println!("spool_rows({noun}): {} 行", rows.len());
    }

    for noun in ["PSPOOL", "PPIECE", "HSPOOL"] {
        let rows = spooling::spool_rows(noun, 1)
            .await
            .expect("spool_rows 失败");
        let Some(row) = rows.first() else {
            println!("{noun}: 库里没有，跳过属性成员检查");
            continue;
        };
        let members = spooling::spool_members(row.refno.into())
            .await
            .unwrap_or_else(|e| panic!("spool_members({}) 失败: {e}", row.refno));
        assert!(
            !members.is_empty(),
            "{noun} {} 一个成员都没有：引用属性没读到",
            row.refno
        );
        for member in &members {
            assert_eq!(member.via, spooling::MemberVia::Attribute);
            assert!(!member.noun.is_empty(), "成员 {} 没补上 noun", member.refno);
        }
        println!(
            "spool_members({noun} {} {}): {}",
            row.refno,
            row.name,
            members
                .iter()
                .map(|m| format!("{} {}", m.noun, m.refno))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // 挑一个有成员的 spool 走一遍成员查询：结构边与引用属性两路都要能跑。
    let mut target: Option<(&str, aios_core::RefU64)> = None;
    for (noun, _) in SPOOL_GROUPS {
        let rows = spooling::spool_rows(noun, 5)
            .await
            .expect("spool_rows 失败");
        if let Some(row) = rows.iter().find(|row| row.children_count > 0) {
            target = Some((noun, row.refno));
            break;
        }
    }
    if let Some((noun, refno)) = target {
        let members = spooling::spool_members(refno.into())
            .await
            .unwrap_or_else(|e| panic!("spool_members({refno}) 失败: {e}"));
        println!(
            "spool_members({} / {}): {} 个成员",
            noun,
            refno,
            members.len()
        );
        for member in &members {
            assert!(
                !member.noun.is_empty(),
                "{refno} 的成员 {} 没补上 noun",
                member.refno
            );
        }
    } else {
        println!("库里没有带成员的 spool，成员查询只验证了语句可执行");
        // 就算没有真实 spool，也要确认这条语句在引擎上不炸：拿一个 PIPE 当靶子，
        // 它没有 SPLMEM / PSARFA 这些属性，回空数组是正确行为。
        if let Some(pipe) = first_pipe().await {
            let members = spooling::spool_members(pipe)
                .await
                .expect("spool_members 对非 spool 元素失败：语句在引擎上跑不通");
            println!("spool_members(PIPE {pipe}): {} 个结构成员", members.len());
        } else {
            println!("库里也没有 PIPE，成员查询整段跳过");
        }
    }

    // 几何翻译的四组断言也在这里跑：`SUL_DB` 是进程级静态句柄，多个
    // `#[tokio::test]` 各建一个 runtime，前一个 runtime 一销毁通道就关，
    // 后面的查询会全报 `closed channel`。所以真库断言共用这一个入口。
    check_geometry_per_family().await;
    check_owner_anchor_is_required().await;
    check_geometry_for_listed_rows().await;
    check_shared_geometry_no_duplicates().await;
    check_non_spool_returns_empty().await;
}

/// 随便取一个 PIPE 当「没有 spool 属性」的对照靶子。
async fn first_pipe() -> Option<aios_core::RefnoEnum> {
    use aios_core::SUL_DB;
    let mut response = SUL_DB
        .query("SELECT VALUE id FROM pe WHERE noun = 'PIPE' AND !deleted LIMIT 1;")
        .await
        .ok()?
        .check()
        .ok()?;
    let rows: Vec<aios_core::RefnoEnum> = response.take(0).ok()?;
    rows.into_iter().next()
}

/// 三族各取一行真实 spool，验证几何翻译非空。
///
/// 这是 VAL-DATA-001 的真库证据：spool 节点自己在 `inst_relate` 里命中 0 行，
/// 几何全靠「成员 ∪ 成员的非 PIPE 父」这条锚点链捞出来。任何一族回空都说明
/// 该族的属性组或锚点链路不对，**不能当成数据现状糊过去**。
///
/// 不单独标 `#[tokio::test]`：`SUL_DB` 是进程级静态句柄，每个 `#[tokio::test]`
/// 各建一个 runtime，前一个测试的 runtime 一销毁通道就关，后面的查询全报
/// `sending into a closed channel`。所有真库断言必须共用同一个 runtime。
async fn check_geometry_per_family() {
    // 每族的取样点：文档 §9 已实测非空（PSPOOL 10 / HSPOOL 40 / PPIECE 31）。
    let samples = [
        ("PSPOOL", "PSPOOL:2013287639_1000"),
        ("PSPOOL", "PSPOOL:2013287639_1001"),
        ("HSPOOL", "HSPOOL:2013287681_407"),
        ("PPIECE", "PPIECE:2013287639_1007"),
    ];
    for (noun, key) in samples {
        let refno: aios_core::RefnoEnum = key.parse::<aios_core::RefU64>().expect("refno").into();
        let started = std::time::Instant::now();
        let geos = spooling::spool_geometry(refno)
            .await
            .unwrap_or_else(|e| panic!("spool_geometry({key}) 失败: {e}"));
        let elapsed = started.elapsed();
        println!(
            "spool_geometry({key}) -> {} 个几何，耗时 {:?}",
            geos.len(),
            elapsed
        );
        assert!(!geos.is_empty(), "{noun} {key} 一个几何都没有：锚点链断了");
        // 非功能要求：单次查询 200ms 内返回。
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "{key} 查询耗时 {elapsed:?}，超过 200ms"
        );
    }
}

/// VAL-DATA-003 的真库证据：锚点必须包含「成员的非 PIPE 父」。
///
/// `PSPOOL:…_1000` 的两个成员是 GASK 垫片，它们自己在 `inst_relate` 里命中 **0** 行；
/// 全部 10 个几何都挂在成员的父（BRAN `…_956`/`…_959` 的上层 `…_089`）下。
/// 所以「只拿成员当锚点」这条退化实现会返回空集，而正确实现返回 10 个。
async fn check_owner_anchor_is_required() {
    use aios_core::SUL_DB;

    // 成员自身当锚点能命中几个。
    let member_only = {
        let sql = "LET $mem = ((SELECT array::flatten([PSARFA ?? [], PSLRFA ?? []]) AS m \
                   FROM ONLY pe:2013287639_1000.refno).m ?? []); \
                   LET $u = array::distinct(array::map($mem, |$x| fn::refno_u64($x))); \
                   RETURN array::len(SELECT VALUE in FROM inst_relate WHERE anc CONTAINSANY $u);";
        let mut response = SUL_DB.query(sql).await.expect("成员锚点查询失败").check().expect("成员锚点查询失败");
        let n: Option<usize> = response.take(2).expect("成员锚点结果反序列化失败");
        n.unwrap_or(0)
    };
    // 成员的非 PIPE 父当锚点能命中几个。
    let owner_only = {
        let sql = "LET $mem = ((SELECT array::flatten([PSARFA ?? [], PSLRFA ?? []]) AS m \
                   FROM ONLY pe:2013287639_1000.refno).m ?? []); \
                   LET $own = array::filter(array::map($mem, |$x| $x.owner), |$o| $o.noun != 'PIPE'); \
                   LET $u = array::distinct(array::map($own, |$o| fn::refno_u64($o))); \
                   RETURN array::len(SELECT VALUE in FROM inst_relate WHERE anc CONTAINSANY $u);";
        let mut response = SUL_DB.query(sql).await.expect("父锚点查询失败").check().expect("父锚点查询失败");
        let n: Option<usize> = response.take(3).expect("父锚点结果反序列化失败");
        n.unwrap_or(0)
    };
    println!("PSPOOL:2013287639_1000 锚点命中：仅成员 {member_only} / 成员非PIPE父 {owner_only}");
    assert_eq!(
        member_only, 0,
        "成员自身居然有几何命中：这个场景不再是「成员无几何」的样本，VAL-DATA-003 证据失效"
    );
    assert!(
        owner_only > 0,
        "成员的非 PIPE 父也没命中几何：锚点链路本身不成立"
    );
}

/// 三族都从 `spool_rows` 取真实行（而不是写死 refno），保证「面板上能看到的行
/// 就能算出几何」——这正是功能有意义的前提。
async fn check_geometry_for_listed_rows() {
    for noun in ["PSPOOL", "HSPOOL", "PPIECE"] {
        let rows = spooling::spool_rows(noun, 3)
            .await
            .unwrap_or_else(|e| panic!("spool_rows({noun}) 失败: {e}"));
        let Some(row) = rows.first() else {
            println!("{noun}: 库里没有，跳过几何检查");
            continue;
        };
        let geos = spooling::spool_geometry(row.refno.into())
            .await
            .unwrap_or_else(|e| panic!("spool_geometry({noun} {}) 失败: {e}", row.refno));
        println!(
            "spool_geometry({noun} {} {}): {} 个几何",
            row.refno,
            row.name,
            geos.len()
        );
        assert!(
            !geos.is_empty(),
            "{noun} {} 在面板上有行却算不出几何：点了眼睛会没反应",
            row.refno
        );
    }
}

/// 已知共享几何的场景：`PSPOOL:…_1000` 与 `_1001` 名下几何集**完全相同**
/// （都是 947/948/949/951/953/954/955/957/958/960）。
///
/// 验证两件事：各自返回集内无重复（VAL-DATA-005），且两行真的指向同一组几何——
/// 后者是面板「隐藏一行、共享几何的另一行跟着变」的依据。
async fn check_shared_geometry_no_duplicates() {
    let a: aios_core::RefnoEnum = "PSPOOL:2013287639_1000"
        .parse::<aios_core::RefU64>()
        .expect("refno")
        .into();
    let b: aios_core::RefnoEnum = "PSPOOL:2013287639_1001"
        .parse::<aios_core::RefU64>()
        .expect("refno")
        .into();
    let geos_a = spooling::spool_geometry(a).await.expect("_1000 几何查询失败");
    let geos_b = spooling::spool_geometry(b).await.expect("_1001 几何查询失败");

    for (name, geos) in [("_1000", &geos_a), ("_1001", &geos_b)] {
        let unique: std::collections::BTreeSet<_> = geos.iter().copied().collect();
        assert_eq!(
            unique.len(),
            geos.len(),
            "{name} 的几何集里有重复：{geos:?}"
        );
        assert!(
            geos.windows(2).all(|w| w[0] < w[1]),
            "{name} 的几何集没排序，顺序不确定：{geos:?}"
        );
    }
    assert_eq!(
        geos_a, geos_b,
        "文档 §7 说这两行共享同一几何集，实测不一致：\n_1000={geos_a:?}\n_1001={geos_b:?}"
    );
    println!("共享几何集（{} 个）：{:?}", geos_a.len(), geos_a);
}

/// 非 spool 元素必须回空。这条在真库上也要成立：`SITE` 下有几百行几何，
/// 但 `SITE` 自己不是 spool，不该把整库都当成它的几何。
async fn check_non_spool_returns_empty() {
    // 走 `spool_rows` 之外的路：先拿真实 SITE 的 refno。
    let site = {
        use aios_core::SUL_DB;
        let mut response = SUL_DB
            .query("SELECT VALUE id FROM pe WHERE noun = 'SITE' AND !deleted LIMIT 1;")
            .await
            .expect("SITE 查询失败")
            .check()
            .expect("SITE 查询失败");
        let rows: Vec<aios_core::RefnoEnum> = response.take(0).expect("SITE 反序列化失败");
        rows.into_iter().next()
    };
    let Some(site) = site else {
        println!("库里没有 SITE，跳过");
        return;
    };
    let geos = spooling::spool_geometry(site)
        .await
        .expect("SITE 的 spool_geometry 不该报错");
    assert!(geos.is_empty(), "SITE {site} 被当成了 spool：{geos:?}");
    println!("spool_geometry(SITE {site}) -> 0 个几何（正确）");

    if let Some(pipe) = first_pipe().await {
        let geos = spooling::spool_geometry(pipe)
            .await
            .expect("PIPE 的 spool_geometry 不该报错");
        assert!(geos.is_empty(), "PIPE {pipe} 被当成了 spool：{geos:?}");
        println!("spool_geometry(PIPE {pipe}) -> 0 个几何（正确）");
    }
}
