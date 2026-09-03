//! 名称子串搜索的本地 ngram 索引（ADR-0023）。
//!
//! 「名字中间含某几个字」在库里无药可救：11 列文档逐行读取本身就是病根，91 万行
//! 上量到 83.6 秒，换写法只在 83~116 秒之间挪。所以子串这一路整个搬出数据库，
//! 落到本地一份 tantivy 索引上——建 152ms、体积 7.4MB、重开 2.2ms、单查亚毫秒
//! （真语料沙箱实测）。
//!
//! 三件事撑起这个模块：
//!
//! 1. **戳**（[`IndexStamp`]）：亚秒可查的陈旧信号，每库一组
//!    `(dbnum, 水位, 行数)`。它散列成目录名——目录在就是新的，不在就得重建，
//!    不需要在索引里另存一份元数据再去比对。
//! 2. **建**（[`build`]）：分库把有名字的行拉回来写进临时目录，commit 之后改名
//!    落位。改名只发生在没有打开句柄的新目录上，Windows 上才不会撞占用。
//! 3. **查**（[`NameIndex::search`]）：针切成 n-gram 全部 AND 出候选池，再拿存下
//!    来的名字**逐条验真**。ngram 索引会漏字序（`ab`+`bc` 也能凑出 `abcbc`），
//!    验真那一步是结果正确的唯一保证，不能省。
//!
//! 整个模块只在原生端存在（`plant-ui-data` 是 wasm 双端 crate）。浏览器端没有
//! 索引，子串路回「仅桌面端提供」，那是调用方的事。

use std::path::{Path, PathBuf};

use aios_core::{RefU64, RefnoEnum, SUL_DB};
use anyhow::{Context, Result};
use serde::Deserialize;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, Query, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term, doc};

use crate::NameHit;

/// 索引格式版本。schema、分词参数、验真口径任一处改动都要推它——它进目录名，
/// 推一格就等于让全世界的旧索引自然失效重建，不需要额外的迁移代码。
const FORMAT: &str = "v1";

/// 分词器名。索引里存的是 schema 引用的名字，打开时必须再注册一次同名同参的
/// 分词器（tokenizer 不随索引落盘）。
const TOKENIZER: &str = "ngram23";
const MIN_GRAM: usize = 2;
const MAX_GRAM: usize = 3;

/// 候选池上限。BM25 只用来挑池子，最终顺序由验真之后的排序键决定（ADR-0023
/// 决定 8）。短针（`-c` 这种）候选会溢出，排序盲区是已知边界。
const CANDIDATES: usize = 200;

/// 写入堆。14 万条名字的语料远够不着这个数，给足只是让 tantivy 一段成型、
/// 少一轮段合并。
const WRITER_HEAP: usize = 64 * 1024 * 1024;

/// 建索引时的临时目录前缀。带 pid 是为了两个进程同时建不会互相踩。
const BUILDING_PREFIX: &str = "building-";

/// 一份打开着的索引。
///
/// 只留 reader 不留 `Index`：`IndexReader` 内部就持有它，再存一份既没人读，也会
/// 多一个把目录钉住的句柄。
pub struct NameIndex {
    reader: IndexReader,
    fields: Fields,
}

#[derive(Debug, Clone, Copy)]
struct Fields {
    name: Field,
    refno: Field,
    noun: Field,
    dbnum: Field,
}

impl Fields {
    fn of(schema: &Schema) -> Result<Self> {
        Ok(Self {
            name: schema.get_field("name")?,
            refno: schema.get_field("refno")?,
            noun: schema.get_field("noun")?,
            dbnum: schema.get_field("dbnum")?,
        })
    }
}

/// 陈旧戳：每个设计库一组 `(dbnum, 水位, 行数)`，按 dbnum 升序。
///
/// - **行数**管增删：加了或删了元素，数就变。
/// - **水位**（`dbnum_watermark.applied_sesno`）管改名：行数不变但内容变了的那一类。
///
/// 两者都够不着的情况客观存在——一个没有水位的库里发生纯改名，戳看不见。那是
/// `reindex` 这个手动动词存在的全部理由，文档与日志都得说这句话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexStamp(Vec<(u32, i32, u64)>);

impl IndexStamp {
    /// 这份戳覆盖的设计库。[`build`] 照它取材，所以索引的范围与戳永远同源，
    /// 不会出现「戳说三个库、索引里只有两个」。
    pub fn dbnums(&self) -> Vec<u32> {
        self.0.iter().map(|(dbnum, _, _)| *dbnum).collect()
    }

    /// 戳覆盖的总行数（含无名行）。只用于日志，不参与判断。
    pub fn rows(&self) -> u64 {
        self.0.iter().map(|(_, _, rows)| *rows).sum()
    }

    /// 索引目录名，形如 `v1-3f2a…`。**目录名即戳**：在就开、不在就建。
    pub fn dir_name(&self) -> String {
        format!("{FORMAT}-{:016x}", self.digest())
    }

    /// FNV-1a 64。不用 `DefaultHasher`：标准库不保证它跨 Rust 版本稳定，换个
    /// 编译器就会让所有人的索引目录名集体改口、白重建一遍。
    fn digest(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut eat = |bytes: &[u8]| {
            for &byte in bytes {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        eat(FORMAT.as_bytes());
        for (dbnum, sesno, rows) in &self.0 {
            eat(&dbnum.to_be_bytes());
            eat(&sesno.to_be_bytes());
            eat(&rows.to_be_bytes());
        }
        hash
    }
}

/// 算一次戳：**一库一条** `count()`，外加一条水位，同一趟往返发出去。
///
/// 一库一条不是啰嗦，是这个查询能不能用的分界线。SurrealDB 只在
/// `WHERE <索引列> = <字面量> GROUP ALL` 这个形状上挑 `IndexCountScan`——数索引
/// 条目，不碰文档。写成一条 `WHERE dbnum IN […] GROUP BY dbnum` 就变成
/// `UnionIndexScan → Filter → Aggregate`，每一行都要把 11 列的文档读出来，
/// 与子串搜索当初被赶出数据库的是同一个病根：AMS 91 万行上 **14.1 秒**，
/// 而分库数是 **0.4 秒量级**（8030 基线库 EXPLAIN + 计时实测，两种形状都量过）。
///
/// dbnum 是 `u32`，拼进 SQL 没有注入面；而这里**必须**是字面量——参数形态
/// （实测 `LET $d = 8000`）会让规划器退回 `Aggregate(IndexScan)`，优化当场消失。
///
/// 水位按 gen-model 自己的读取顺序解析（`applied_sesno` 优先，退回旧的 `sesno`，
/// 都没有算 0）——只认 `applied_sesno` 的话，一个还没迁移过的老库会永远报 0，
/// 改名就彻底看不见了。整张表**读不到就当全 0**：没跑过 gen-model 的库里根本
/// 没有 `dbnum_watermark` 这张表，那时行数仍然管得住增删，不该让整个戳失败。
pub async fn stamp(dbnums: &[u32]) -> Result<IndexStamp> {
    // 先排序去重：一库一条语句，重复的 dbnum 就是白发一条；排序则是散列稳定的
    // 前提——设计库名单取自一条查询，没人保证它每次的顺序一样。
    let mut wanted = dbnums.to_vec();
    wanted.sort_unstable();
    wanted.dedup();
    if wanted.is_empty() {
        return Ok(IndexStamp(Vec::new()));
    }

    #[derive(Deserialize)]
    struct CountRow {
        count: u64,
    }

    let mut sql = String::with_capacity(wanted.len() * 64);
    for dbnum in &wanted {
        sql.push_str(&format!(
            "SELECT count() FROM pe WHERE dbnum = {dbnum} GROUP ALL;"
        ));
    }
    sql.push_str("SELECT VALUE [dbnum, applied_sesno ?? sesno ?? 0] FROM dbnum_watermark;");

    let mut response = SUL_DB.query(sql).await.context("读取索引陈旧戳失败")?;
    let mut stamp: Vec<(u32, i32, u64)> = Vec::with_capacity(wanted.len());
    for (index, dbnum) in wanted.iter().enumerate() {
        let counted: Vec<CountRow> = response
            .take(index)
            .with_context(|| format!("设计库 {dbnum} 的行数反序列化失败"))?;
        // 空库的两种回法：`IndexCountScan` 回 `{count: 0}`，没有索引时退化成的
        // 那条路回空集。都是 0 行。
        stamp.push((*dbnum, 0, counted.first().map_or(0, |row| row.count)));
    }
    let marks: Vec<(u32, i32)> = response.take(wanted.len()).unwrap_or_default();
    for (dbnum, sesno) in marks {
        if let Some(row) = stamp.iter_mut().find(|(num, _, _)| *num == dbnum) {
            row.1 = sesno;
        }
    }
    Ok(IndexStamp(stamp))
}

/// 打开一份已经落位的索引。2.2ms 量级。
///
/// **失败当成「不存在」**是调用方的口径：目录残缺、格式对不上、被别的进程写坏，
/// 一律重建一份就是了，没有需要抢救的东西。
pub fn open(dir: &Path) -> Result<NameIndex> {
    let index =
        Index::open_in_dir(dir).with_context(|| format!("打开索引目录失败：{}", dir.display()))?;
    register_tokenizer(&index)?;
    let fields = Fields::of(&index.schema()).context("索引 schema 与本版不符")?;
    // Manual：这份索引的写入方是我们自己，而且写完就换新目录，没有「同一目录里
    // 又提交了一版」的情形。省掉 meta.json 的文件监听线程。
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()
        .context("建立索引读取器失败")?;
    Ok(NameIndex { reader, fields })
}

/// 建一份新索引，落位到 `root/<戳>/`，然后打开它。
///
/// 拉语料与写索引分成两步（[`load_corpus`] + [`write`]），不是为了好看：写入那
/// 一段全是同步调用，分开之后 `Index` / `IndexWriter` 不跨 `.await`，这个 future
/// 才是 `Send` 的，才进得了 `tokio::spawn`。语料 14 万行 6MB 级，整份拿在手上
/// 不心疼。
pub async fn build(
    root: &Path,
    stamp: &IndexStamp,
    progress: impl FnMut(usize, usize),
) -> Result<NameIndex> {
    let corpus = load_corpus(&stamp.dbnums(), progress).await?;
    write(root, stamp, &corpus)
}

/// 把一份语料写成索引并落位。同步、几百毫秒（14 万行实测 152ms）。
///
/// 落位靠改名：写进 `building-<pid>`，commit 并等合并线程收工（句柄全放掉）之后
/// 才改名成 `<戳>`。绝不在打开中的索引目录上原地写。
pub fn write(root: &Path, stamp: &IndexStamp, corpus: &[NameHit]) -> Result<NameIndex> {
    std::fs::create_dir_all(root)
        .with_context(|| format!("建索引根目录失败：{}", root.display()))?;
    let target = root.join(stamp.dir_name());
    let temp = root.join(format!("{BUILDING_PREFIX}{}", std::process::id()));
    if temp.exists() {
        std::fs::remove_dir_all(&temp)
            .with_context(|| format!("清理上次的半成品失败：{}", temp.display()))?;
    }
    std::fs::create_dir_all(&temp)
        .with_context(|| format!("建临时索引目录失败：{}", temp.display()))?;

    {
        let (schema, fields) = schema();
        let index = Index::create_in_dir(&temp, schema).context("创建索引失败")?;
        register_tokenizer(&index)?;
        let mut writer: IndexWriter<TantivyDocument> =
            index.writer(WRITER_HEAP).context("创建索引写入器失败")?;
        for row in corpus {
            writer
                .add_document(doc!(
                    fields.name => row.name.as_str(),
                    fields.refno => row.refno.0,
                    fields.noun => row.noun.as_str(),
                    fields.dbnum => u64::from(row.dbnum),
                ))
                .context("写入索引文档失败")?;
        }
        writer.commit().context("提交索引失败")?;
        // 消费掉 writer：合并线程收工、锁文件放掉，目录才改得动。
        writer
            .wait_merging_threads()
            .context("等待索引合并线程失败")?;
    }

    // 目录名即戳，所以「目标已存在」多半是别的进程抢先落位了同一份。但强制重建
    // （`reindex`）走的也是这条路，而它存在的全部理由正是**戳看不见的改名**——
    // 那时新建的这份才是对的，旧的必须让位。
    if target.exists() {
        let _ = std::fs::remove_dir_all(&target);
    }
    if target.exists() {
        // 删不掉：有人（可能就是本进程）还开着它。退回用旧的那份——少刷新一次
        // 胜过让整次重建报错。强制重建的调用方要先把自己的句柄放掉。
        let _ = std::fs::remove_dir_all(&temp);
    } else {
        std::fs::rename(&temp, &target)
            .with_context(|| format!("索引落位失败：{} -> {}", temp.display(), target.display()))?;
    }
    sweep(root, &target);
    open(&target)
}

/// 拉语料：分库取有名字的行。
///
/// **必须分库**。一次性拉 91 万行会压断 WS 通道；分库最大单库 36 万行 8.5 秒，
/// 全程 31.4 秒（真库实测）。过滤交给服务端——两条都是纯比较，不是函数调用，
/// 不会像 `string::contains` 那样把 dbnum 索引一起废掉。
///
/// **`name != NONE` 一条不够**：没名字的元素在 `pe` 里存的是**空串**，不是 NONE
/// （真库实测，无名行占绝大多数——91 万行里只有 14.3 万行真有名字）。少了
/// `name != ''` 就会把几十万条空名字拉回来喂进索引，既撑大索引也让每一条空名字
/// 都成为潜在的假命中。
///
/// 也供集成探针取「逐行比对」的地面真值用：同一份语料，一边进索引一边线性扫，
/// 两边对不上就是索引这一层的账。
pub async fn load_corpus(
    dbnums: &[u32],
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<NameHit>> {
    let total = dbnums.len();
    let mut done = 0;
    progress(done, total);
    let mut corpus = Vec::new();
    for &dbnum in dbnums {
        let mut response = SUL_DB
            .query(
                "SELECT VALUE [id, name, noun ?? ''] FROM pe \
                 WHERE dbnum = $dbnum AND name != NONE AND name != ''",
            )
            .bind(("dbnum", dbnum))
            .await
            .with_context(|| format!("取设计库 {dbnum} 的名字失败"))?;
        let rows: Vec<(RefnoEnum, String, String)> = response
            .take(0)
            .with_context(|| format!("设计库 {dbnum} 的名字反序列化失败"))?;
        corpus.extend(rows.into_iter().map(|(refno, name, noun)| NameHit {
            refno: refno.refno(),
            name,
            noun,
            dbnum,
        }));
        done += 1;
        progress(done, total);
    }
    Ok(corpus)
}

impl NameIndex {
    /// 索引里的名字条数。日志用（「子串索引已就绪：143,244 个名字」）。
    pub fn len(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 找名字里含 `needle` 的元素。亚毫秒。
    ///
    /// 三步：切 n-gram 全部 AND 出候选池 → 拿存下来的名字**逐条验真** → 按
    /// ADR-0023 决定 8 排序（段首命中 > 段中，再名字短优先，再字典序）。
    ///
    /// 报错而不是回空集：查询炸了和「一条都没有」在界面上长得一模一样，而这两件
    /// 事该说的话完全不同。
    pub fn search(&self, needle: &str, limit: usize) -> Result<Vec<NameHit>> {
        let needle = needle.trim().to_lowercase();
        let grams = grams(&needle);
        if grams.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let clauses = grams
            .iter()
            .map(|gram| {
                let term = Term::from_field_text(self.fields.name, gram);
                let query = TermQuery::new(term, IndexRecordOption::Basic);
                (Occur::Must, Box::new(query) as Box<dyn Query>)
            })
            .collect::<Vec<_>>();
        let searcher = self.reader.searcher();
        // `TopDocs` 在 0.26 只是个建造器，`order_by_score()` 才是那个 collector。
        let candidates = searcher
            .search(
                &BooleanQuery::new(clauses),
                &TopDocs::with_limit(CANDIDATES).order_by_score(),
            )
            .context("子串索引查询失败")?;

        let mut ranked = Vec::new();
        for (_score, address) in candidates {
            let document: TantivyDocument = searcher.doc(address).context("读取索引文档失败")?;
            let Some(hit) = self.to_hit(&document) else {
                continue;
            };
            let lowered = hit.name.to_lowercase();
            let Some(rank) = rank_of(&lowered, &needle) else {
                // ngram 命中不等于真的含这个串（`ab` + `bc` 也能凑出 `abcbc`）。
                continue;
            };
            ranked.push((rank, hit.name.chars().count(), hit));
        }
        ranked.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.name.cmp(&right.2.name))
        });
        ranked.truncate(limit);
        Ok(ranked.into_iter().map(|(_, _, hit)| hit).collect())
    }

    fn to_hit(&self, document: &TantivyDocument) -> Option<NameHit> {
        Some(NameHit {
            refno: RefU64(document.get_first(self.fields.refno)?.as_u64()?),
            name: document.get_first(self.fields.name)?.as_str()?.to_owned(),
            noun: document
                .get_first(self.fields.noun)
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_owned(),
            dbnum: document.get_first(self.fields.dbnum)?.as_u64()? as u32,
        })
    }
}

/// schema。改这里就要推 [`FORMAT`]。
///
/// `name` 是唯一被索引的字段：`IndexRecordOption::Basic` 不存词位置——AND 之后
/// 反正要逐条验真，位置信息买不到任何东西，只会把索引撑大。
fn schema() -> (Schema, Fields) {
    let mut builder = Schema::builder();
    let indexing = TextFieldIndexing::default()
        .set_tokenizer(TOKENIZER)
        .set_index_option(IndexRecordOption::Basic);
    let name = builder.add_text_field(
        "name",
        TextOptions::default()
            .set_indexing_options(indexing)
            .set_stored(),
    );
    // refno 存 `RefU64` 的 packed 原值：字符串形态要解析才能用回去，而这一列
    // 每条命中都要读。
    let refno = builder.add_u64_field("refno", STORED);
    let noun = builder.add_text_field("noun", STRING | STORED);
    let dbnum = builder.add_u64_field("dbnum", STORED);
    (
        builder.build(),
        Fields {
            name,
            refno,
            noun,
            dbnum,
        },
    )
}

/// 分词器不随索引落盘，建和开都得注册一次，参数必须一模一样。
fn register_tokenizer(index: &Index) -> Result<()> {
    let ngram = NgramTokenizer::all_ngrams(MIN_GRAM, MAX_GRAM).context("构造 ngram 分词器失败")?;
    index.tokenizers().register(
        TOKENIZER,
        TextAnalyzer::builder(ngram).filter(LowerCaser).build(),
    );
    Ok(())
}

/// 把针切成查询用的 gram。
///
/// 针够长就全部走 3-gram（区分度高、候选池收敛快），2 个字的针只有一个 2-gram。
/// 比 [`MIN_GRAM`] 还短的针一个 gram 都切不出来——那正是「最短针 2 字符」这条
/// 决定的由来，不是随手定的下限。
fn grams(needle: &str) -> Vec<String> {
    let chars: Vec<char> = needle.chars().collect();
    if chars.len() < MIN_GRAM {
        return Vec::new();
    }
    let size = chars.len().min(MAX_GRAM);
    let mut grams: Vec<String> = Vec::new();
    for window in chars.windows(size) {
        let gram: String = window.iter().collect();
        if !grams.contains(&gram) {
            grams.push(gram);
        }
    }
    grams
}

/// 验真兼定级：`None` = 名字里根本没有这个串；`Some(0)` = 有一处落在分段开头；
/// `Some(1)` = 只在段中出现。
///
/// 段界 = 名字头，或 `/` `-` `_` 之后。`/A-RS-C1` 里 `rs-c` 是段首（前面是 `-`），
/// `s-c` 是段中（前面是 `r`）——人心里想的「以这段开头」就是这个意思。
fn rank_of(lowered_name: &str, needle: &str) -> Option<u8> {
    let mut found = None;
    for (at, _) in lowered_name.match_indices(needle) {
        let boundary = match lowered_name[..at].chars().next_back() {
            None => true,
            Some(previous) => matches!(previous, '/' | '-' | '_'),
        };
        if boundary {
            return Some(0);
        }
        found = Some(1);
    }
    found
}

/// 扫掉同级的其他目录：上一版索引、别的进程留下的半成品。
///
/// **删不掉就算了**。Windows 上另一个进程可能正开着上一版索引，删除会失败；
/// 那只是多占一份磁盘，下次启动再扫一遍就好，绝不该因此让重建报错。
fn sweep(root: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if path == keep || !path.is_dir() {
            continue;
        }
        let _ = std::fs::remove_dir_all(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_needles_degrade_to_one_whole_gram() {
        assert_eq!(grams("r"), Vec::<String>::new(), "比 2 个字还短切不出 gram");
        assert_eq!(grams("rs"), vec!["rs"], "两个字的针只有一个 2-gram");
        assert_eq!(grams("rs-"), vec!["rs-"], "三个字的针就是它自己");
        assert_eq!(grams("rs-c1"), vec!["rs-", "s-c", "-c1"]);
        // 重复的 gram 只留一份：AND 里多一遍同样的词毫无意义。
        assert_eq!(grams("ababa"), vec!["aba", "bab"]);
    }

    #[test]
    fn a_hit_at_a_segment_head_outranks_one_in_the_middle() {
        let name = "/a-rs-c1";
        assert_eq!(rank_of(name, "rs-c"), Some(0), "`-` 之后是段首");
        assert_eq!(rank_of(name, "s-c"), Some(1), "`r` 之后是段中");
        assert_eq!(rank_of(name, "/a"), Some(0), "名字头本身就是段界");
        assert_eq!(
            rank_of(name, "a-r"),
            Some(0),
            "PDMS 全名的前导 `/` 也是段界"
        );
        assert_eq!(rank_of(name, "s-"), Some(1), "跨过分隔符不等于从分隔符起头");
        assert_eq!(rank_of(name, "zz"), None, "没有就是没有");
        assert_eq!(rank_of("/p_rs_c", "rs"), Some(0), "`_` 也是段界");
        // 一处段中、一处段首 -> 按最好的那处算。
        assert_eq!(rank_of("/xrs-rs1", "rs"), Some(0));
    }

    #[test]
    fn verification_rejects_what_ngrams_alone_would_accept() {
        // `abcbc` 的 2/3-gram 覆盖了 `abc` 与 `bcb`，但它并不含 `acb`。
        assert_eq!(rank_of("abcbc", "acb"), None);
    }

    #[test]
    fn ranking_goes_segment_head_then_short_name_then_alphabetical() {
        let mut rows = vec![
            (Some(1u8), 4usize, "/b-xrs"),
            (Some(0), 6, "/rs-long"),
            (Some(0), 4, "/rs-b"),
            (Some(0), 4, "/rs-a"),
        ];
        rows.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(right.2))
        });
        let names: Vec<&str> = rows.iter().map(|row| row.2).collect();
        assert_eq!(names, vec!["/rs-a", "/rs-b", "/rs-long", "/b-xrs"]);
    }

    #[test]
    fn the_stamp_digest_is_pinned_and_notices_every_change() {
        let base = IndexStamp(vec![(7997, 41, 360_000), (7999, 0, 12)]);
        // 钉死的期望值：这个数一变，全世界的索引目录名就跟着改口。要改先想清楚。
        assert_eq!(base.dir_name(), "v1-6062805ba5976594");
        assert_eq!(base.dbnums(), vec![7997, 7999]);
        assert_eq!(base.rows(), 360_012);

        let more_rows = IndexStamp(vec![(7997, 41, 360_001), (7999, 0, 12)]);
        let newer_mark = IndexStamp(vec![(7997, 42, 360_000), (7999, 0, 12)]);
        let one_db_gone = IndexStamp(vec![(7997, 41, 360_000)]);
        for other in [&more_rows, &newer_mark, &one_db_gone] {
            assert_ne!(base.dir_name(), other.dir_name());
        }
    }

    #[test]
    fn an_empty_db_list_still_has_a_stable_stamp() {
        // 没有设计库时不该 panic，也不该和任何真实戳撞名。
        let empty = IndexStamp(Vec::new());
        assert!(empty.dir_name().starts_with("v1-"));
        assert_eq!(empty.dbnums(), Vec::<u32>::new());
        assert_eq!(empty.rows(), 0);
    }
}
