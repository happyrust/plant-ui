//! 名称子串索引的真库探针（常驻）。
//!
//! 单元测试钉的是切 gram、判段界、排序键这些纯逻辑；这里钉的是**它在真数据上
//! 说的是不是实话**——索引查出来的那一批，与同一份语料逐行 `contains` 扫出来的
//! 那一批，必须一条不差。ngram 索引天生会放进假命中（`ab`+`bc` 能凑出 `abcbc`），
//! 验真那一步一旦写坏，单元测试全绿而界面开始撒谎，只有这条探针拦得住。
//!
//! 顺带把 ADR-0023 里那四个数在本机重新量一遍：算戳 / 拉语料 / 建索引 / 重开 /
//! 单查。数字进日志，方案文档里的量级说法才有据可依。
//!
//! 前置条件：工作目录（workspace 根）有指向本地库的 `DbOption.toml`。
//!
//! 可选的第二道对拍：`PLANT_UI_PROBE_DB_PARITY=1` 会另外跑一条库内逐行
//! `string::contains` 查询来对答案。它是**分钟级**的（91 万行实测 83.6 秒，那正是
//! 这个索引存在的理由），所以默认不跑；改了取语料那条 SQL 的时候值得跑一次。

use std::path::Path;
use std::time::{Duration, Instant};

use aios_core::{RefU64, SUL_DB};
use plant_ui_data::{NameHit, name_index};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "需要本地 SurrealDB（DbOption.toml），用 --ignored 显式跑"]
async fn probe_name_index_parity_and_timings() {
    // cargo 给集成测试的 CWD 是 crate 目录，而 DbOption.toml 在 workspace 根。
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
    plant_ui_data::connect().await.expect("连接本地库失败");
    let (project, mdb, ns, dbnums) = plant_ui_data::project_identity()
        .await
        .expect("project_identity 失败");
    println!("project={project} mdb={mdb} ns={ns} desi_dbs={dbnums:?}");
    assert!(
        !dbnums.is_empty(),
        "当前 MDB 一个设计库都没有，探针无从谈起"
    );

    // 一、戳。它挂在每一次启动、每一次取回工作的路上，慢一点都不行。
    let started = Instant::now();
    let stamp = name_index::stamp(&dbnums).await.expect("算戳失败");
    let stamp_cost = started.elapsed();
    println!(
        "戳 {}：{} 个库 {} 行，{stamp_cost:?}",
        stamp.dir_name(),
        stamp.dbnums().len(),
        stamp.rows()
    );
    assert_eq!(stamp.dbnums(), sorted(&dbnums), "戳漏了库或多了库");
    assert!(stamp.rows() > 0, "戳报出 0 行，取行数那条 SQL 没生效");
    assert!(
        stamp_cost < Duration::from_secs(5),
        "算戳花了 {stamp_cost:?}——它是每个触发点都要跑的亚秒级查询，不能退化成扫表"
    );
    // 同一份数据算两次必须一模一样，否则每次启动都会白重建一遍索引。
    let again = name_index::stamp(&dbnums).await.expect("再算一次戳失败");
    assert_eq!(stamp, again, "戳不稳定");

    // 二、语料。分库拉，服务端过滤掉无名行。
    let started = Instant::now();
    let corpus = name_index::load_corpus(&stamp.dbnums(), |done, total| {
        if done > 0 {
            println!("  语料 {done}/{total} 库");
        }
    })
    .await
    .expect("拉语料失败");
    let load_cost = started.elapsed();
    println!("语料 {} 个名字，{load_cost:?}", corpus.len());
    assert!(!corpus.is_empty(), "一个有名字的元素都没拉到");
    assert!(
        corpus.iter().all(|row| !row.name.is_empty()),
        "语料里混进了无名行——无名元素存的是空串不是 NONE，服务端少了 `name != ''`"
    );

    // 服务端过滤要与「整库拉回来自己筛」等价。拿行数最少的那个库对一遍：
    // 这条 SQL 换了写法就得重跑这里，漏行是静默的，界面上只表现为「搜不到」。
    verify_server_side_filter(&stamp, &corpus).await;

    // 三、写索引 + 落位。
    let root = std::env::temp_dir().join("plant-ui-name-index-probe");
    let _ = std::fs::remove_dir_all(&root);
    let started = Instant::now();
    let index = name_index::write(&root, &stamp, &corpus).expect("写索引失败");
    println!("建索引 {:?}（不含拉语料）", started.elapsed());
    assert_eq!(index.len(), corpus.len() as u64, "索引条数与语料对不上");
    drop(index);

    let dir = root.join(stamp.dir_name());
    assert!(dir.is_dir(), "索引没有落位到 {}", dir.display());
    // 落位之后同级只该剩它一个：半成品目录必须被扫掉。
    let siblings: Vec<String> = std::fs::read_dir(&root)
        .expect("读索引根目录失败")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(siblings, vec![stamp.dir_name()], "索引根目录里有残留");
    println!("索引体积 {:.1} MB", dir_size(&dir) as f64 / 1024.0 / 1024.0);

    // 四、重开。冷启动走的就是这一条，2ms 量级是它值得存在的理由。
    let started = Instant::now();
    let index = name_index::open(&dir).expect("重开索引失败");
    println!("重开 {:?}", started.elapsed());
    assert_eq!(index.len(), corpus.len() as u64, "重开后条数变了");

    // 五、对拍。针取三种：方案里点名的 `rs-c`、语料里现挑的一段真名字中间片段、
    // 一个几乎必然一条都不中的串。三种都必须与逐行扫的结果一字不差。
    let middle = middle_fragment(&corpus).expect("语料里挑不出一个够长的名字");
    for needle in ["rs-c", middle.as_str(), "zqx-nothing"] {
        let started = Instant::now();
        let hits = index.search(needle, 10_000).expect("查索引失败");
        let cost = started.elapsed();
        let truth = scan(&corpus, needle);
        println!(
            "「{needle}」-> 索引 {} 条 / 逐行 {} 条，{cost:?}",
            hits.len(),
            truth.len()
        );

        if truth.len() > 200 {
            // 候选池封顶 200（ADR-0023 决定 8 的已知边界），这种针对不了全集，
            // 只能验「回来的每一条都是真的」。
            assert!(hits.len() <= 200, "候选池上限失守：{}", hits.len());
            for hit in &hits {
                assert!(
                    truth.contains(&hit.refno),
                    "索引回了一条名字里没有「{needle}」的：{}",
                    hit.name
                );
            }
            continue;
        }
        assert_eq!(
            sorted_refnos(&hits),
            sorted(&truth),
            "「{needle}」对不上：索引 {:?} / 逐行 {:?}",
            hits.iter().map(|hit| hit.name.as_str()).collect::<Vec<_>>(),
            truth.len()
        );
        assert!(
            hits.iter().all(|hit| stamp.dbnums().contains(&hit.dbnum)),
            "命中跑出了索引范围"
        );
        assert!(
            cost < Duration::from_millis(50),
            "单查花了 {cost:?}，慢得不像索引"
        );

        // 排序：段首命中必须整段排在段中命中前面。
        let heads: Vec<bool> = hits
            .iter()
            .map(|hit| starts_a_segment(&hit.name, needle))
            .collect();
        assert!(
            heads.windows(2).all(|pair| pair[0] >= pair[1]),
            "「{needle}」的排序把段中命中排到了段首命中前面"
        );
    }

    if std::env::var("PLANT_UI_PROBE_DB_PARITY").as_deref() == Ok("1") {
        db_parity(&index, &stamp.dbnums(), &middle).await;
    } else {
        println!("（库内逐行对拍已跳过：设 PLANT_UI_PROBE_DB_PARITY=1 跑，分钟级）");
    }

    drop(index);
    let _ = std::fs::remove_dir_all(&root);
}

/// 服务端的 `name != NONE AND name != ''` 与「整库拉回来客户端筛空串」必须给出
/// 同一批行。
///
/// 只对行数最少的那个库做——这是一次不加过滤的整库拉，大库上要好几秒。
async fn verify_server_side_filter(stamp: &name_index::IndexStamp, corpus: &[NameHit]) {
    let Some(dbnum) = smallest_db(stamp).await else {
        return;
    };
    let mut response = SUL_DB
        .query("SELECT VALUE [id, name ?? '', noun ?? ''] FROM pe WHERE dbnum = $dbnum")
        .bind(("dbnum", dbnum))
        .await
        .expect("整库拉取失败");
    let rows: Vec<(aios_core::RefnoEnum, String, String)> =
        response.take(0).expect("整库反序列化失败");
    let mut client_side: Vec<RefU64> = rows
        .into_iter()
        .filter(|(_, name, _)| !name.is_empty())
        .map(|(refno, _, _)| refno.refno())
        .collect();
    client_side.sort_unstable();
    let server_side = sorted_refnos(
        &corpus
            .iter()
            .filter(|row| row.dbnum == dbnum)
            .cloned()
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        server_side, client_side,
        "设计库 {dbnum}：服务端过滤与客户端过滤拉出来的行不一样"
    );
    println!(
        "服务端过滤对拍通过（设计库 {dbnum}，{} 个名字）",
        client_side.len()
    );
}

/// 库内逐行 `string::contains`：索引诞生之前的那条路，也是最终的地面真值。
/// 分钟级，靠环境变量显式开。
async fn db_parity(index: &name_index::NameIndex, dbnums: &[u32], needle: &str) {
    let started = Instant::now();
    let mut response = SUL_DB
        .query(
            "SELECT VALUE refno FROM \
             (SELECT id AS refno, name, dbnum FROM pe WHERE dbnum IN $dbnums) \
             WHERE string::contains(string::lowercase(name ?? ''), $needle)",
        )
        .bind(("dbnums", dbnums.to_vec()))
        .bind(("needle", needle.to_lowercase()))
        .await
        .expect("库内逐行查询失败");
    let rows: Vec<aios_core::RefnoEnum> = response.take(0).expect("库内结果反序列化失败");
    let mut truth: Vec<RefU64> = rows.into_iter().map(|refno| refno.refno()).collect();
    truth.sort_unstable();
    println!(
        "库内逐行「{needle}」-> {} 条，{:?}",
        truth.len(),
        started.elapsed()
    );

    let hits = index.search(needle, 10_000).expect("查索引失败");
    assert_eq!(sorted_refnos(&hits), truth, "索引与库内逐行的结果对不上");
}

/// 行数最少的设计库。整库拉一遍的那种对拍只该落在它头上。
async fn smallest_db(stamp: &name_index::IndexStamp) -> Option<u32> {
    let dbnums = stamp.dbnums();
    #[derive(serde::Deserialize)]
    struct Row {
        dbnum: u32,
        rows: u64,
    }
    let mut response = SUL_DB
        .query("SELECT dbnum, count() AS rows FROM pe WHERE dbnum IN $dbnums GROUP BY dbnum")
        .bind(("dbnums", dbnums))
        .await
        .ok()?;
    let rows: Vec<Row> = response.take(0).ok()?;
    rows.into_iter()
        .min_by_key(|row| row.rows)
        .map(|row| row.dbnum)
}

/// 语料里挑一段「名字中间」的真片段：够长的名字掐掉头两个字取四个。
fn middle_fragment(corpus: &[NameHit]) -> Option<String> {
    corpus
        .iter()
        .find(|row| row.name.chars().count() >= 8)
        .map(|row| {
            row.name
                .chars()
                .skip(3)
                .take(4)
                .collect::<String>()
                .to_lowercase()
        })
}

fn scan(corpus: &[NameHit], needle: &str) -> Vec<RefU64> {
    let needle = needle.to_lowercase();
    let mut hits: Vec<RefU64> = corpus
        .iter()
        .filter(|row| row.name.to_lowercase().contains(&needle))
        .map(|row| row.refno)
        .collect();
    hits.sort_unstable();
    hits
}

fn starts_a_segment(name: &str, needle: &str) -> bool {
    let name = name.to_lowercase();
    let needle = needle.to_lowercase();
    name.match_indices(&needle).any(|(at, _)| {
        name[..at]
            .chars()
            .next_back()
            .is_none_or(|previous| matches!(previous, '/' | '-' | '_'))
    })
}

fn sorted_refnos(hits: &[NameHit]) -> Vec<RefU64> {
    let mut refnos: Vec<RefU64> = hits.iter().map(|hit| hit.refno).collect();
    refnos.sort_unstable();
    refnos
}

fn sorted<T: Ord + Clone>(items: &[T]) -> Vec<T> {
    let mut items = items.to_vec();
    items.sort_unstable();
    items
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum()
}
