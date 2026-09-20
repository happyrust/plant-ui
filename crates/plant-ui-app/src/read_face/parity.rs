//! 对拍探针（计划 §5.4；D8 / D14）：`plant-ui-app --read-face-parity`。
//!
//! 同一进程里各起一份服务供数与库供数的读面，对着同一个接入点把树、属性、三维实例各读
//! 一遍，逐项比出差异——**只出报告、不判对错**：服务读文件最新、库读水位，两边时点可以
//! 不同，报告头把两枚凭证（`/dbnums` 的 `file_latest_sesno` / `applied_sesno`）写出来，
//! 读的人自己对时点。
//!
//! 这是**唯一**允许两面并存的进程形态，而且不进 UI：D2 禁的是同一视口里两版数据悄悄拼在
//! 一起，无头子命令没有视口。不进 CI（要活的 gen-model 与 SurrealDB）；
//! `scripts/Run-ReadFaceParity.ps1` 一键跑。
//!
//! 比法：
//! - 树：roots 集合 diff，然后逐层 `children` 按 refno **集合 + 原序**比（成员序是语义，
//!   gen-model spec §6.5.1）；只往两边都有的节点底下走。
//! - 属性：从两边共有的节点里等距抽 `--sample` 个，逐字段比，四档照 gen-model
//!   `direct-mode-expression-dialect.md` §1.1 的口径：相同 / 只差首尾空白 / 只差括号与空格 /
//!   不同；库供数回空表的元素（元件库元素、未同步）单独列。
//! - 三维实例：抽样根各读 `model_instances`，按库分桶比 `refno + geo_hash` 集合。库号从
//!   根的搜索命中解（refno 高 32 位是 db ref 不是库号，客户端算不出），解不出的桶按 db ref 列。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use futures::StreamExt;
use plant_ui::task_queue::DbnumStatus;
use plant_ui_data::{Attr, EleTreeNode, RefU64};

use super::{Identity, ModelInstancesReq, ReadFace, ReadFaceKind, ServiceIdentity};
use crate::data::SEARCH_LIMIT;
use crate::logs::error_chain;
use crate::model_update_api;
use crate::search_index::{Scope, SearchIndex};

/// 命令行上认这一个开关；其余参数都跟在它后面。
pub const FLAG: &str = "--read-face-parity";

pub const USAGE: &str = "用法：plant-ui-app --read-face-parity [--depth N] [--sample N] \
[--roots 24381/2,24381/3] [--out report.md] [--service http://127.0.0.1:8022]\n\
  --depth    往下比几层子节点（0 = 只比 SITE 根层），默认 2\n\
  --sample   抽多少个两边都有的元素比属性，默认 200\n\
  --roots    三维实例的抽样根（逗号分隔的 refno）；不给就从第 1 层共有节点里等距抽\n\
  --out      报告写到这个文件（Markdown）；不给就打到标准输出\n\
  --service  模型服务地址，压过设置文件与 PLANT_MODEL_API_URL";

/// 报告里每类差异最多逐条列多少行；超出只计数。
const LIST_CAP: usize = 50;
/// 不给 `--roots` 时三维实例最多抽几个根。
const INSTANCE_ROOTS: usize = 10;
/// 两面并发读时同时在途的节点数。
const IN_FLIGHT: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub depth: usize,
    pub sample: usize,
    pub roots: Vec<RefU64>,
    pub out: Option<PathBuf>,
    pub service: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            depth: 2,
            sample: 200,
            roots: Vec::new(),
            out: None,
            service: None,
        }
    }
}

/// 解命令行。`--key value` 与 `--key=value` 都认；认不出的开关直接报错，不悄悄忽略。
pub fn parse_args<I>(args: I) -> anyhow::Result<Options>
where
    I: IntoIterator<Item = String>,
{
    let mut options = Options::default();
    let mut args = args.into_iter().peekable();
    while let Some(arg) = args.next() {
        if arg == FLAG {
            continue;
        }
        let (key, inline_value) = match arg.split_once('=') {
            Some((key, value)) => (key.to_owned(), Some(value.to_owned())),
            None => (arg.clone(), None),
        };
        let mut value = || -> anyhow::Result<String> {
            if let Some(value) = inline_value.clone() {
                return Ok(value);
            }
            args.next()
                .filter(|next| !next.starts_with("--"))
                .with_context(|| format!("{key} 后面要跟一个值\n{USAGE}"))
        };
        match key.as_str() {
            "--depth" => {
                let raw = value()?;
                options.depth = raw
                    .trim()
                    .parse()
                    .with_context(|| format!("--depth 要一个非负整数，给的是 {raw:?}"))?;
            }
            "--sample" => {
                let raw = value()?;
                options.sample = raw
                    .trim()
                    .parse()
                    .with_context(|| format!("--sample 要一个非负整数，给的是 {raw:?}"))?;
            }
            "--roots" => {
                let raw = value()?;
                for piece in raw
                    .split(',')
                    .map(str::trim)
                    .filter(|piece| !piece.is_empty())
                {
                    let refno: RefU64 = piece
                        .parse()
                        .map_err(|_| anyhow::anyhow!("--roots 里认不出 refno：{piece:?}"))?;
                    options.roots.push(refno);
                }
            }
            "--out" => options.out = Some(PathBuf::from(value()?)),
            "--service" => options.service = Some(value()?.trim().trim_end_matches('/').to_owned()),
            "--help" | "-h" => anyhow::bail!("{USAGE}"),
            other => anyhow::bail!("认不出的参数：{other}\n{USAGE}"),
        }
    }
    Ok(options)
}

/// 报告里点名一个元素用的三样。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub refno: RefU64,
    pub noun: String,
    pub name: String,
}

impl Node {
    fn of(node: &EleTreeNode) -> Self {
        Self {
            refno: node.refno.refno(),
            noun: node.noun.clone(),
            name: node.name.clone(),
        }
    }

    fn bare(refno: RefU64) -> Self {
        Self {
            refno,
            noun: String::new(),
            name: String::new(),
        }
    }

    /// `SITE /1RB-CIVI（=24381/2）`；没名字的只留类型与 refno。
    pub fn label(&self) -> String {
        let mut label = String::new();
        if !self.noun.is_empty() {
            label.push_str(&self.noun);
            label.push(' ');
        }
        if !self.name.is_empty() {
            label.push_str(&self.name);
            label.push(' ');
        }
        label.push_str(&format!("(={})", self.refno.to_slash_string()));
        label
    }
}

/// 一次查询失败：哪一面、查什么、错误链。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub side: ReadFaceKind,
    pub what: String,
    pub error: String,
}

/// 同一个父节点底下两边子层的差异。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SeqDiff {
    pub common: Vec<Node>,
    pub only_service: Vec<Node>,
    pub only_store: Vec<Node>,
    /// 两边共有的成员按各自原序排出来是否一致。集合不同也比——只看共有那部分。
    pub same_order: bool,
}

/// 集合 + 原序。重复的 refno 只认第一次出现。
pub fn diff_children(service: &[EleTreeNode], store: &[EleTreeNode]) -> SeqDiff {
    fn dedup(nodes: &[EleTreeNode]) -> Vec<Node> {
        let mut seen = HashSet::new();
        nodes
            .iter()
            .map(Node::of)
            .filter(|node| seen.insert(node.refno))
            .collect()
    }
    let service = dedup(service);
    let store = dedup(store);
    let service_set: HashSet<RefU64> = service.iter().map(|node| node.refno).collect();
    let store_set: HashSet<RefU64> = store.iter().map(|node| node.refno).collect();
    let common: Vec<Node> = service
        .iter()
        .filter(|node| store_set.contains(&node.refno))
        .cloned()
        .collect();
    let store_order: Vec<RefU64> = store
        .iter()
        .filter(|node| service_set.contains(&node.refno))
        .map(|node| node.refno)
        .collect();
    let service_order: Vec<RefU64> = common.iter().map(|node| node.refno).collect();
    SeqDiff {
        same_order: service_order == store_order,
        only_service: service
            .iter()
            .filter(|node| !store_set.contains(&node.refno))
            .cloned()
            .collect(),
        only_store: store
            .iter()
            .filter(|node| !service_set.contains(&node.refno))
            .cloned()
            .collect(),
        common,
    }
}

/// 一个父节点底下共有成员的两种排法。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderMismatch {
    pub parent: Option<Node>,
    pub service: Vec<String>,
    pub store: Vec<String>,
}

/// 树的一层：第 0 层是 SITE 根层（父节点是 MDB 世界，记 1 个），往下每层的父节点是上一层
/// 两边共有的节点。
#[derive(Debug, Default)]
pub struct LevelReport {
    pub parents: usize,
    pub service: usize,
    pub store: usize,
    pub common: usize,
    pub only_service: Vec<(Option<Node>, Node)>,
    pub only_store: Vec<(Option<Node>, Node)>,
    pub order_mismatches: Vec<OrderMismatch>,
    pub failures: Vec<Failure>,
}

impl LevelReport {
    fn absorb(
        &mut self,
        parent: Option<&Node>,
        service: &[EleTreeNode],
        store: &[EleTreeNode],
    ) -> Vec<Node> {
        let diff = diff_children(service, store);
        self.service += diff.common.len() + diff.only_service.len();
        self.store += diff.common.len() + diff.only_store.len();
        self.common += diff.common.len();
        self.only_service.extend(
            diff.only_service
                .into_iter()
                .map(|node| (parent.cloned(), node)),
        );
        self.only_store.extend(
            diff.only_store
                .into_iter()
                .map(|node| (parent.cloned(), node)),
        );
        if !diff.same_order {
            let store_order: Vec<String> = {
                let common: HashSet<RefU64> = diff.common.iter().map(|node| node.refno).collect();
                let mut seen = HashSet::new();
                store
                    .iter()
                    .map(Node::of)
                    .filter(|node| common.contains(&node.refno) && seen.insert(node.refno))
                    .map(|node| node.label())
                    .collect()
            };
            self.order_mismatches.push(OrderMismatch {
                parent: parent.cloned(),
                service: diff.common.iter().map(Node::label).collect(),
                store: store_order,
            });
        }
        diff.common
    }

    pub fn has_differences(&self) -> bool {
        !self.only_service.is_empty()
            || !self.only_store.is_empty()
            || !self.order_mismatches.is_empty()
            || !self.failures.is_empty()
    }
}

/// 属性值的四档（gen-model `direct-mode-expression-dialect.md` §1.1 的口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ValueVerdict {
    /// 逐字节相同。
    Same,
    /// 只差首尾空白。
    TrimOnly,
    /// 去掉全部括号与空白后完全相等。
    ParenSpaceOnly,
    /// 以上都不是。
    Different,
}

impl ValueVerdict {
    pub const ALL: [Self; 4] = [
        Self::Same,
        Self::TrimOnly,
        Self::ParenSpaceOnly,
        Self::Different,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Same => "相同",
            Self::TrimOnly => "只差首尾空白",
            Self::ParenSpaceOnly => "只差括号与空格",
            Self::Different => "不同",
        }
    }
}

/// 先比逐字节、再比 trim、再比「去掉全部括号与空白」，剩下的就是不同。
pub fn classify_value(service: &str, store: &str) -> ValueVerdict {
    if service == store {
        return ValueVerdict::Same;
    }
    if service.trim() == store.trim() {
        return ValueVerdict::TrimOnly;
    }
    fn strip(text: &str) -> String {
        text.chars()
            .filter(|c| !c.is_whitespace() && !matches!(c, '(' | ')'))
            .collect()
    }
    if strip(service) == strip(store) {
        return ValueVerdict::ParenSpaceOnly;
    }
    ValueVerdict::Different
}

/// 「不同」那一档的一条。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDiff {
    pub node: Node,
    pub field: String,
    pub service: String,
    pub store: String,
}

#[derive(Debug, Default)]
pub struct PropsReport {
    /// 抽了多少个元素。
    pub sampled: usize,
    /// 两边都回了非空表、逐字段比过的元素数。
    pub compared: usize,
    pub fields: BTreeMap<ValueVerdict, usize>,
    pub only_service_fields: usize,
    pub only_store_fields: usize,
    /// 服务有表、库供数空表：元件库元素（不入模型本体库）或没同步到的元素，这里不猜是哪种。
    pub store_empty: Vec<Node>,
    /// 库有表、服务空表。
    pub service_empty: Vec<Node>,
    pub both_empty: usize,
    /// 「不同」逐条（最多 [`LIST_CAP`] 条，计数在 `fields` 里）。
    pub differences: Vec<FieldDiff>,
    /// 「不同」按字段名计数——两面对同一个值的写法不一样（refno 的分隔符、方位串、
    /// 未设值）时，差异会整齐地落在几个字段上，这张表一眼看得出是写法还是数据。
    pub different_by_field: BTreeMap<String, usize>,
    /// 只在一边有的字段名样例（最多 [`LIST_CAP`] 条）。
    pub one_sided_fields: Vec<(Node, ReadFaceKind, String)>,
    /// 只在服务 / 只在库有的字段，按字段名计数。
    pub only_service_by_field: BTreeMap<String, usize>,
    pub only_store_by_field: BTreeMap<String, usize>,
    pub failures: Vec<Failure>,
}

/// 计数表按次数倒序、同次数按名字，取前 `cap` 行。
fn top_fields(counts: &BTreeMap<String, usize>, cap: usize) -> Vec<(&str, usize)> {
    let mut rows: Vec<(&str, usize)> = counts
        .iter()
        .map(|(field, count)| (field.as_str(), *count))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    rows.truncate(cap);
    rows
}

impl PropsReport {
    fn field_count(&self, verdict: ValueVerdict) -> usize {
        self.fields.get(&verdict).copied().unwrap_or_default()
    }

    /// 一个元素两张表逐字段归档。
    pub fn compare(&mut self, node: &Node, service: &[Attr], store: &[Attr]) {
        match (service.is_empty(), store.is_empty()) {
            (true, true) => {
                self.both_empty += 1;
                return;
            }
            (false, true) => {
                self.store_empty.push(node.clone());
                return;
            }
            (true, false) => {
                self.service_empty.push(node.clone());
                return;
            }
            (false, false) => {}
        }
        self.compared += 1;
        let store_by_name: HashMap<&str, &Attr> = store
            .iter()
            .map(|attr| (attr.name.as_str(), attr))
            .collect();
        let mut seen = HashSet::new();
        for attr in service {
            seen.insert(attr.name.as_str());
            match store_by_name.get(attr.name.as_str()) {
                Some(other) => {
                    let verdict = classify_value(&attr.value, &other.value);
                    *self.fields.entry(verdict).or_default() += 1;
                    if verdict == ValueVerdict::Different {
                        *self
                            .different_by_field
                            .entry(attr.name.clone())
                            .or_default() += 1;
                        if self.differences.len() < LIST_CAP {
                            self.differences.push(FieldDiff {
                                node: node.clone(),
                                field: attr.name.clone(),
                                service: attr.value.clone(),
                                store: other.value.clone(),
                            });
                        }
                    }
                }
                None => {
                    self.only_service_fields += 1;
                    *self
                        .only_service_by_field
                        .entry(attr.name.clone())
                        .or_default() += 1;
                    if self.one_sided_fields.len() < LIST_CAP {
                        self.one_sided_fields.push((
                            node.clone(),
                            ReadFaceKind::Service,
                            attr.name.clone(),
                        ));
                    }
                }
            }
        }
        for attr in store {
            if !seen.contains(attr.name.as_str()) {
                self.only_store_fields += 1;
                *self
                    .only_store_by_field
                    .entry(attr.name.clone())
                    .or_default() += 1;
                if self.one_sided_fields.len() < LIST_CAP {
                    self.one_sided_fields.push((
                        node.clone(),
                        ReadFaceKind::Store,
                        attr.name.clone(),
                    ));
                }
            }
        }
    }
}

/// 三维实例按库分的一桶。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Bucket {
    pub service: usize,
    pub store: usize,
    pub common: usize,
    pub only_service: usize,
    pub only_store: usize,
    /// 差异样例 `refno#geo_hash`（最多 [`LIST_CAP`] 条）。
    pub samples: Vec<String>,
}

/// 一条实例的身份：`(refno, geo_hash)`；没有网格实例的记录 geo_hash 为空串，只算元素本身。
pub type InstanceKey = (RefU64, String);

pub fn instance_keys(records: &[aios_core::GeomInstQuery]) -> Vec<InstanceKey> {
    let mut keys = Vec::new();
    for record in records {
        let refno = record.refno.refno();
        if record.insts.is_empty() {
            keys.push((refno, String::new()));
        }
        for inst in &record.insts {
            keys.push((refno, inst.geo_hash.clone()));
        }
    }
    keys
}

/// 桶名：解得出库号的 `db7997`，解不出的按 refno 高 32 位列 `ref24381（dbnum 未解）`。
fn bucket_label(refno: RefU64, dbnum_of_ref: &HashMap<u32, u32>) -> String {
    match dbnum_of_ref.get(&refno.get_0()) {
        Some(dbnum) => format!("db{dbnum}"),
        None => format!("ref{}（dbnum 未解）", refno.get_0()),
    }
}

/// 两边的实例集合按桶比。
pub fn bucket_instances(
    service: &[InstanceKey],
    store: &[InstanceKey],
    dbnum_of_ref: &HashMap<u32, u32>,
) -> BTreeMap<String, Bucket> {
    let service: HashSet<&InstanceKey> = service.iter().collect();
    let store: HashSet<&InstanceKey> = store.iter().collect();
    let mut buckets: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut sorted: Vec<&InstanceKey> = service.union(&store).copied().collect();
    sorted.sort();
    for key in sorted {
        let bucket = buckets
            .entry(bucket_label(key.0, dbnum_of_ref))
            .or_default();
        let in_service = service.contains(key);
        let in_store = store.contains(key);
        if in_service {
            bucket.service += 1;
        }
        if in_store {
            bucket.store += 1;
        }
        match (in_service, in_store) {
            (true, true) => bucket.common += 1,
            (true, false) => {
                bucket.only_service += 1;
                if bucket.samples.len() < LIST_CAP {
                    bucket.samples.push(format!(
                        "只在服务：={}#{}",
                        key.0.to_slash_string(),
                        key.1
                    ));
                }
            }
            (false, true) => {
                bucket.only_store += 1;
                if bucket.samples.len() < LIST_CAP {
                    bucket
                        .samples
                        .push(format!("只在库：={}#{}", key.0.to_slash_string(), key.1));
                }
            }
            (false, false) => unreachable!("键来自两边的并集"),
        }
    }
    buckets
}

#[derive(Debug, Default)]
pub struct InstanceReport {
    pub roots: Vec<Node>,
    /// 根的 refno 高 32 位 → 库号（从搜索命中解出来的）。
    pub dbnum_of_ref: HashMap<u32, u32>,
    pub buckets: BTreeMap<String, Bucket>,
    pub failures: Vec<Failure>,
}

/// 从 `total` 个里等距抽 `count` 个的下标；不够抽就全要。
pub fn evenly_spaced(total: usize, count: usize) -> Vec<usize> {
    if count == 0 || total == 0 {
        return Vec::new();
    }
    if count >= total {
        return (0..total).collect();
    }
    (0..count).map(|i| i * total / count).collect()
}

/// `/dbnums` 里一行的两枚凭证。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub dbnum: u32,
    pub db_type: String,
    pub applied_sesno: i32,
    pub file_latest_sesno: i32,
    pub excluded: bool,
    pub not_in_project: bool,
}

impl From<&DbnumStatus> for Credential {
    fn from(row: &DbnumStatus) -> Self {
        Self {
            dbnum: row.dbnum,
            db_type: row.db_type.clone(),
            applied_sesno: row.applied_sesno,
            file_latest_sesno: row.file_latest_sesno,
            excluded: row.excluded,
            not_in_project: row.not_in_project,
        }
    }
}

pub struct Report {
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub elapsed_secs: f64,
    pub options: Options,
    pub service_base: String,
    pub store_endpoint: String,
    pub data_face: String,
    pub service_identity: Identity,
    pub store_identity: Identity,
    pub credentials: Vec<Credential>,
    pub levels: Vec<LevelReport>,
    pub props: PropsReport,
    pub instances: InstanceReport,
}

impl Report {
    /// 打在标准错误上的一句收尾。
    pub fn summary_line(&self) -> String {
        let roots = &self.levels[0];
        format!(
            "roots 服务 {} / 库 {} / 共有 {}（只在服务 {}、只在库 {}、原序{}）；属性抽样 {}，不同 {} 字段；实例 {} 个桶，{} 个桶有差异",
            roots.service,
            roots.store,
            roots.common,
            roots.only_service.len(),
            roots.only_store.len(),
            if roots.order_mismatches.is_empty() {
                "一致"
            } else {
                "不一致"
            },
            self.props.sampled,
            self.props.field_count(ValueVerdict::Different),
            self.instances.buckets.len(),
            self.instances
                .buckets
                .values()
                .filter(|bucket| bucket.only_service + bucket.only_store > 0)
                .count(),
        )
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let o = &mut out;
        let push = |o: &mut String, line: &str| {
            o.push_str(line);
            o.push('\n');
        };
        push(
            o,
            &format!(
                "# 供数模式对拍报告 · {} {}",
                self.service_identity.project, self.service_identity.mdb
            ),
        );
        push(o, "");
        push(
            o,
            "> 差异只列不判：服务供数读文件最新、库供数读水位，两边时点可以不同。对时点看下面两枚凭证。",
        );
        push(o, "");
        push(
            o,
            &format!(
                "- 生成：{}（用时 {:.1} s）",
                self.generated_at.format("%Y-%m-%d %H:%M:%S UTC"),
                self.elapsed_secs
            ),
        );
        push(o, &format!("- 服务供数：`{}`", self.service_base));
        push(o, &format!("- 库供数：`{}`", self.store_endpoint));
        let roots_note = if self.options.roots.is_empty() {
            format!(
                "从第 {} 层共有节点里等距抽 ≤ {INSTANCE_ROOTS} 个",
                self.options.depth.min(1)
            )
        } else {
            self.options
                .roots
                .iter()
                .map(|refno| format!("={}", refno.to_slash_string()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        push(
            o,
            &format!(
                "- 参数：`--depth {}` `--sample {}`；三维实例抽样根：{}",
                self.options.depth, self.options.sample, roots_note
            ),
        );
        push(o, "");

        push(o, "## 凭证（`/dbnums`）");
        push(o, "");
        push(
            o,
            &format!(
                "- 服务端数据形态 `data_face = {}`。`applied_sesno` 是库里应用到的会话号（库供数读的水位），`file_latest_sesno` 是文件此刻自报的最新会话号（服务供数读的时点）。两数不同的库，属性与实例的差异不算缺陷。",
                self.data_face
            ),
        );
        push(o, "");
        push(
            o,
            "| dbnum | 类型 | applied_sesno（库水位） | file_latest_sesno（文件最新） | 差 | 备注 |",
        );
        push(o, "|---|---|---:|---:|---:|---|");
        for row in &self.credentials {
            let note = match (row.excluded, row.not_in_project) {
                (_, true) => "MDB 声明了、项目目录里没有",
                (true, _) => "不在本期范围",
                _ => "",
            };
            push(
                o,
                &format!(
                    "| {} | {} | {} | {} | {} | {} |",
                    row.dbnum,
                    row.db_type,
                    row.applied_sesno,
                    row.file_latest_sesno,
                    row.file_latest_sesno - row.applied_sesno,
                    note
                ),
            );
        }
        push(o, "");

        push(o, "## 工程标识");
        push(o, "");
        push(o, "| 格 | 服务供数 | 库供数 | 一致 |");
        push(o, "|---|---|---|---|");
        let s = &self.service_identity;
        let t = &self.store_identity;
        let mark = |same: bool| if same { "是" } else { "**否**" };
        push(
            o,
            &format!(
                "| project | {} | {} | {} |",
                s.project,
                t.project,
                mark(s.project == t.project)
            ),
        );
        push(
            o,
            &format!("| mdb | {} | {} | {} |", s.mdb, t.mdb, mark(s.mdb == t.mdb)),
        );
        push(
            o,
            &format!("| ns | {} | {} | {} |", s.ns, t.ns, mark(s.ns == t.ns)),
        );
        let nums = |nums: &[u32]| {
            nums.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut s_nums = s.db_nums.clone();
        let mut t_nums = t.db_nums.clone();
        s_nums.sort_unstable();
        t_nums.sort_unstable();
        push(
            o,
            &format!(
                "| 设计库 | {} | {} | {} |",
                nums(&s_nums),
                nums(&t_nums),
                mark(s_nums == t_nums)
            ),
        );
        push(o, "");

        push(o, "## 树");
        push(o, "");
        push(
            o,
            "| 层 | 比过的父节点 | 服务 | 库 | 共有 | 只在服务 | 只在库 | 原序不一致的父节点 | 查询失败 |",
        );
        push(o, "|---|---:|---:|---:|---:|---:|---:|---:|---:|");
        for (depth, level) in self.levels.iter().enumerate() {
            let name = if depth == 0 {
                "0（SITE 根层）".to_owned()
            } else {
                depth.to_string()
            };
            push(
                o,
                &format!(
                    "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                    name,
                    level.parents,
                    level.service,
                    level.store,
                    level.common,
                    level.only_service.len(),
                    level.only_store.len(),
                    level.order_mismatches.len(),
                    level.failures.len()
                ),
            );
        }
        push(o, "");
        let roots = &self.levels[0];
        let verdict = if roots.only_service.is_empty()
            && roots.only_store.is_empty()
            && roots.order_mismatches.is_empty()
        {
            "roots 集合与原序**零差异**。"
        } else {
            "roots 集合或原序**有差异**（硬标准没过；两边库的数据本身是否同源先看凭证与设计库那一行）。"
        };
        push(o, verdict);
        push(o, "");
        for (depth, level) in self.levels.iter().enumerate() {
            if !level.has_differences() {
                continue;
            }
            push(o, &format!("### 第 {depth} 层差异"));
            push(o, "");
            let parent_label = |parent: &Option<Node>| {
                parent
                    .as_ref()
                    .map(Node::label)
                    .unwrap_or_else(|| "MDB 世界".to_owned())
            };
            for (parent, node) in level.only_service.iter().take(LIST_CAP) {
                push(
                    o,
                    &format!("- 只在服务：{} ← {}", node.label(), parent_label(parent)),
                );
            }
            if level.only_service.len() > LIST_CAP {
                push(
                    o,
                    &format!(
                        "- …只在服务的还有 {} 条",
                        level.only_service.len() - LIST_CAP
                    ),
                );
            }
            for (parent, node) in level.only_store.iter().take(LIST_CAP) {
                push(
                    o,
                    &format!("- 只在库：{} ← {}", node.label(), parent_label(parent)),
                );
            }
            if level.only_store.len() > LIST_CAP {
                push(
                    o,
                    &format!("- …只在库的还有 {} 条", level.only_store.len() - LIST_CAP),
                );
            }
            for mismatch in level.order_mismatches.iter().take(LIST_CAP) {
                push(
                    o,
                    &format!("- 原序不一致 ← {}", parent_label(&mismatch.parent)),
                );
                push(o, &format!("  - 服务：{}", mismatch.service.join(" → ")));
                push(o, &format!("  - 库：{}", mismatch.store.join(" → ")));
            }
            if level.order_mismatches.len() > LIST_CAP {
                push(
                    o,
                    &format!(
                        "- …原序不一致的还有 {} 个父节点",
                        level.order_mismatches.len() - LIST_CAP
                    ),
                );
            }
            for failure in &level.failures {
                push(
                    o,
                    &format!(
                        "- 查询失败（{}）：{}：{}",
                        failure.side.label(),
                        failure.what,
                        failure.error
                    ),
                );
            }
            push(o, "");
        }

        push(o, "## 属性");
        push(o, "");
        let p = &self.props;
        push(
            o,
            &format!(
                "抽样 {} 个两边都有的元素：逐字段比了 {} 个；库供数空表 {} 个（元件库元素或未同步，不猜是哪种）；服务空表 {} 个；两边都空 {} 个；查询失败 {} 次。",
                p.sampled,
                p.compared,
                p.store_empty.len(),
                p.service_empty.len(),
                p.both_empty,
                p.failures.len()
            ),
        );
        push(o, "");
        push(o, "| 档 | 字段数 |");
        push(o, "|---|---:|");
        for verdict in ValueVerdict::ALL {
            push(
                o,
                &format!("| {} | {} |", verdict.label(), p.field_count(verdict)),
            );
        }
        push(
            o,
            &format!("| 只在服务有的字段 | {} |", p.only_service_fields),
        );
        push(o, &format!("| 只在库有的字段 | {} |", p.only_store_fields));
        push(o, "");
        if !p.store_empty.is_empty() {
            push(o, "### 库供数空表的元素");
            push(o, "");
            for node in p.store_empty.iter().take(LIST_CAP) {
                push(o, &format!("- {}", node.label()));
            }
            if p.store_empty.len() > LIST_CAP {
                push(o, &format!("- …还有 {} 个", p.store_empty.len() - LIST_CAP));
            }
            push(o, "");
        }
        if !p.service_empty.is_empty() {
            push(o, "### 服务供数空表的元素");
            push(o, "");
            for node in p.service_empty.iter().take(LIST_CAP) {
                push(o, &format!("- {}", node.label()));
            }
            push(o, "");
        }
        if !p.different_by_field.is_empty() {
            push(o, "### 「不同」按字段");
            push(o, "");
            push(
                o,
                "同一个字段在抽样里几乎每个元素都不同，多半是两面写法不一样（refno 的分隔符、方位串、未设值的写法），不是数据不一样；只落在个别元素上的才值得对着凭证看。",
            );
            push(o, "");
            push(o, "| 字段 | 不同的元素数 |");
            push(o, "|---|---:|");
            for (field, count) in top_fields(&p.different_by_field, LIST_CAP) {
                push(o, &format!("| `{}` | {} |", cell(field), count));
            }
            if p.different_by_field.len() > LIST_CAP {
                push(
                    o,
                    &format!(
                        "| …还有 {} 个字段 | |",
                        p.different_by_field.len() - LIST_CAP
                    ),
                );
            }
            push(o, "");
        }
        if !p.differences.is_empty() {
            push(
                o,
                &format!("### 「不同」明细（前 {} 条）", p.differences.len()),
            );
            push(o, "");
            push(o, "| 元素 | 字段 | 服务供数 | 库供数 |");
            push(o, "|---|---|---|---|");
            for diff in &p.differences {
                push(
                    o,
                    &format!(
                        "| {} | {} | `{}` | `{}` |",
                        diff.node.label(),
                        diff.field,
                        cell(&diff.service),
                        cell(&diff.store)
                    ),
                );
            }
            push(o, "");
        }
        for (side, counts) in [
            (ReadFaceKind::Service, &p.only_service_by_field),
            (ReadFaceKind::Store, &p.only_store_by_field),
        ] {
            if counts.is_empty() {
                continue;
            }
            push(
                o,
                &format!(
                    "### 只在{}有的字段（{} 个字段名，按出现的元素数）",
                    side.label(),
                    counts.len()
                ),
            );
            push(o, "");
            push(o, "| 字段 | 元素数 |");
            push(o, "|---|---:|");
            for (field, count) in top_fields(counts, LIST_CAP) {
                push(o, &format!("| `{}` | {} |", cell(field), count));
            }
            if counts.len() > LIST_CAP {
                push(
                    o,
                    &format!("| …还有 {} 个字段 | |", counts.len() - LIST_CAP),
                );
            }
            push(o, "");
        }
        if !p.one_sided_fields.is_empty() {
            push(
                o,
                &format!(
                    "### 只在一边有的字段样例（前 {} 条）",
                    p.one_sided_fields.len()
                ),
            );
            push(o, "");
            for (node, side, field) in &p.one_sided_fields {
                push(
                    o,
                    &format!("- 只在{}：{} · `{}`", side.label(), node.label(), field),
                );
            }
            push(o, "");
        }
        for failure in &p.failures {
            push(
                o,
                &format!(
                    "- 查询失败（{}）：{}：{}",
                    failure.side.label(),
                    failure.what,
                    failure.error
                ),
            );
        }
        if !p.failures.is_empty() {
            push(o, "");
        }

        push(o, "## 三维实例");
        push(o, "");
        let i = &self.instances;
        if i.roots.is_empty() {
            push(
                o,
                "没有可抽的根（两边共有的节点为空，或 `--depth 0` 且没给 `--roots`）。",
            );
            push(o, "");
        } else {
            push(
                o,
                &format!(
                    "抽样根 {} 个：{}",
                    i.roots.len(),
                    i.roots
                        .iter()
                        .map(Node::label)
                        .collect::<Vec<_>>()
                        .join("、")
                ),
            );
            push(o, "");
            push(
                o,
                "键 = `refno + geo_hash`，按集合比（同一元素下重复的键只算一次）；桶按库号分，库号由根的搜索命中解出（refno 高 32 位是 db ref，不是库号），解不出的桶按 db ref 列。",
            );
            push(o, "");
            push(o, "| 桶 | 服务 | 库 | 共有 | 只在服务 | 只在库 |");
            push(o, "|---|---:|---:|---:|---:|---:|");
            for (label, bucket) in &i.buckets {
                push(
                    o,
                    &format!(
                        "| {} | {} | {} | {} | {} | {} |",
                        label,
                        bucket.service,
                        bucket.store,
                        bucket.common,
                        bucket.only_service,
                        bucket.only_store
                    ),
                );
            }
            if i.buckets.is_empty() {
                push(o, "| （两边都没读到实例） | 0 | 0 | 0 | 0 | 0 |");
            }
            push(o, "");
            for (label, bucket) in &i.buckets {
                if bucket.samples.is_empty() {
                    continue;
                }
                push(
                    o,
                    &format!("### {label} 差异样例（前 {} 条）", bucket.samples.len()),
                );
                push(o, "");
                for sample in &bucket.samples {
                    push(o, &format!("- {sample}"));
                }
                push(o, "");
            }
        }
        for failure in &i.failures {
            push(
                o,
                &format!(
                    "- 查询失败（{}）：{}：{}",
                    failure.side.label(),
                    failure.what,
                    failure.error
                ),
            );
        }
        if !i.failures.is_empty() {
            push(o, "");
        }
        out
    }
}

/// 表格单元格里不能有竖线与换行。
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\r', '\n'], "⏎")
}

/// 跑一遍。两面任一连不上就整体失败——对拍要两边都在场。
pub async fn run(options: &Options) -> anyhow::Result<Report> {
    let started = Instant::now();
    let service = ReadFace::new(ReadFaceKind::Service);
    let store = ReadFace::new(ReadFaceKind::Store);
    let base = model_update_api::base_url();
    eprintln!("[对拍] 服务供数 {base}；库供数连库…");

    let (service_identity, store_identity, dbnums) = futures::join!(
        service.identity(),
        store.identity(),
        model_update_api::dbnum_report(&base)
    );
    let service_identity =
        service_identity.context("服务供数取工程标识失败（模型服务在场吗？）")?;
    let store_identity = store_identity.context("库供数取工程标识失败（SurrealDB 在场吗？）")?;
    let dbnums = dbnums.context("读 /dbnums 失败")?;
    let db = aios_core::get_db_option();
    let store_endpoint = format!(
        "{}:{} ns {} / {}",
        db.v_ip, db.v_port, store_identity.ns, store_identity.mdb
    );
    eprintln!(
        "[对拍] 两面都在：服务 {} {} ns {}；库 {} {} ns {}",
        service_identity.project,
        service_identity.mdb,
        service_identity.ns,
        store_identity.project,
        store_identity.mdb,
        store_identity.ns
    );

    // 树：根层，然后逐层往两边共有的节点底下走。
    let (service_roots, store_roots) = futures::join!(service.sites(), store.sites());
    let service_roots = service_roots.context("服务供数取根层失败")?;
    let store_roots = store_roots.context("库供数取根层失败")?;
    let mut levels = Vec::new();
    let mut root_level = LevelReport {
        parents: 1,
        ..Default::default()
    };
    let mut frontier = root_level.absorb(None, &service_roots, &store_roots);
    eprintln!(
        "[对拍] roots：服务 {} / 库 {} / 共有 {}",
        root_level.service, root_level.store, root_level.common
    );
    levels.push(root_level);
    let mut common_nodes: Vec<Node> = frontier.clone();
    let mut level_one: Vec<Node> = Vec::new();
    for depth in 1..=options.depth {
        let mut level = LevelReport {
            parents: frontier.len(),
            ..Default::default()
        };
        let results: Vec<(
            usize,
            Node,
            anyhow::Result<Vec<EleTreeNode>>,
            anyhow::Result<Vec<EleTreeNode>>,
        )> = futures::stream::iter(frontier.iter().cloned().enumerate())
            .map(|(index, parent)| {
                let service = &service;
                let store = &store;
                async move {
                    let (s, t) = futures::join!(
                        service.children(parent.refno),
                        store.children(parent.refno)
                    );
                    (index, parent, s, t)
                }
            })
            .buffer_unordered(IN_FLIGHT)
            .collect()
            .await;
        let mut results = results;
        results.sort_by_key(|(index, ..)| *index);
        let mut next = Vec::new();
        for (_, parent, s, t) in results {
            match (s, t) {
                (Ok(s), Ok(t)) => next.extend(level.absorb(Some(&parent), &s, &t)),
                (s, t) => {
                    if let Err(error) = s {
                        level.failures.push(Failure {
                            side: ReadFaceKind::Service,
                            what: format!("children {}", parent.label()),
                            error: error_chain(&error),
                        });
                    }
                    if let Err(error) = t {
                        level.failures.push(Failure {
                            side: ReadFaceKind::Store,
                            what: format!("children {}", parent.label()),
                            error: error_chain(&error),
                        });
                    }
                }
            }
        }
        eprintln!(
            "[对拍] 第 {depth} 层：{} 个父节点，子节点服务 {} / 库 {} / 共有 {}，原序不一致 {}",
            level.parents,
            level.service,
            level.store,
            level.common,
            level.order_mismatches.len()
        );
        if depth == 1 {
            level_one = next.clone();
        }
        common_nodes.extend(next.iter().cloned());
        frontier = next;
        levels.push(level);
    }

    // 属性：从共有节点里等距抽样。
    let scope = Scope {
        project: service_identity.project.clone(),
        ns: service_identity.ns.clone(),
        mdb: service_identity.mdb.clone(),
        dbnums: service_identity.db_nums.clone(),
        cache_versions: service_identity.cache_versions.clone(),
    };
    let picks: Vec<Node> = evenly_spaced(common_nodes.len(), options.sample)
        .into_iter()
        .map(|index| common_nodes[index].clone())
        .collect();
    let mut props = PropsReport {
        sampled: picks.len(),
        ..Default::default()
    };
    let results: Vec<(
        usize,
        Node,
        anyhow::Result<Vec<Attr>>,
        anyhow::Result<Vec<Attr>>,
    )> = futures::stream::iter(picks.into_iter().enumerate())
        .map(|(index, node)| {
            let service = &service;
            let store = &store;
            let scope = &scope;
            async move {
                let (s, t) = futures::join!(
                    service.props(node.refno, scope),
                    store.props(node.refno, scope)
                );
                (index, node, s, t)
            }
        })
        .buffer_unordered(IN_FLIGHT)
        .collect()
        .await;
    let mut results = results;
    results.sort_by_key(|(index, ..)| *index);
    for (_, node, s, t) in results {
        match (s, t) {
            (Ok(s), Ok(t)) => props.compare(&node, &s, &t),
            (s, t) => {
                if let Err(error) = s {
                    props.failures.push(Failure {
                        side: ReadFaceKind::Service,
                        what: format!("props {}", node.label()),
                        error: error_chain(&error),
                    });
                }
                if let Err(error) = t {
                    props.failures.push(Failure {
                        side: ReadFaceKind::Store,
                        what: format!("props {}", node.label()),
                        error: error_chain(&error),
                    });
                }
            }
        }
    }
    eprintln!(
        "[对拍] 属性：抽样 {}，比了 {}，不同 {} 字段，库供数空表 {}",
        props.sampled,
        props.compared,
        props.field_count(ValueVerdict::Different),
        props.store_empty.len()
    );

    // 三维实例：抽样根，先解库号，再按桶比。
    let roots: Vec<Node> = if options.roots.is_empty() {
        let pool = if options.depth == 0 {
            &common_nodes
        } else {
            &level_one
        };
        evenly_spaced(pool.len(), INSTANCE_ROOTS)
            .into_iter()
            .map(|index| pool[index].clone())
            .collect()
    } else {
        options.roots.iter().copied().map(Node::bare).collect()
    };
    let mut instances = InstanceReport {
        roots: roots.clone(),
        ..Default::default()
    };
    let index = SearchIndex::default();
    for root in &roots {
        if root.name.is_empty() || instances.dbnum_of_ref.contains_key(&root.refno.get_0()) {
            continue;
        }
        let outcome = service.search(&root.name, SEARCH_LIMIT, &index).await;
        let hit = match outcome.prefix {
            Ok(hits) => hits.into_iter().find(|hit| hit.refno == root.refno),
            Err(_) => {
                let outcome = store.search(&root.name, SEARCH_LIMIT, &index).await;
                outcome
                    .prefix
                    .ok()
                    .and_then(|hits| hits.into_iter().find(|hit| hit.refno == root.refno))
            }
        };
        if let Some(hit) = hit {
            instances.dbnum_of_ref.insert(root.refno.get_0(), hit.dbnum);
        }
    }
    let identity = ServiceIdentity {
        base: &base,
        project: &service_identity.project,
        mdb: &service_identity.mdb,
        namespace: &service_identity.ns,
    };
    let mut service_keys = Vec::new();
    let mut store_keys = Vec::new();
    for root in &roots {
        let one = [root.refno];
        let req = ModelInstancesReq {
            roots: &one,
            generation_roots: &one,
            identity,
        };
        let mut quiet_service = |_: usize, _: usize| {};
        let mut quiet_store = |_: usize, _: usize| {};
        let (s, t) = futures::join!(
            service.model_instances(&req, &mut quiet_service),
            store.model_instances(&req, &mut quiet_store)
        );
        match s {
            Ok(records) => service_keys.extend(instance_keys(&records.records)),
            Err(error) => instances.failures.push(Failure {
                side: ReadFaceKind::Service,
                what: format!("model_instances {}", root.label()),
                error: error_chain(&error),
            }),
        }
        match t {
            Ok(records) => store_keys.extend(instance_keys(&records.records)),
            Err(error) => instances.failures.push(Failure {
                side: ReadFaceKind::Store,
                what: format!("model_instances {}", root.label()),
                error: error_chain(&error),
            }),
        }
    }
    instances.buckets = bucket_instances(&service_keys, &store_keys, &instances.dbnum_of_ref);
    eprintln!(
        "[对拍] 实例：{} 个根，服务 {} / 库 {} 条键（去重前），{} 个桶",
        roots.len(),
        service_keys.len(),
        store_keys.len(),
        instances.buckets.len()
    );

    Ok(Report {
        generated_at: chrono::Utc::now(),
        elapsed_secs: started.elapsed().as_secs_f64(),
        options: options.clone(),
        service_base: base,
        store_endpoint,
        data_face: dbnums.data_face.clone(),
        service_identity,
        store_identity,
        credentials: dbnums.dbnums.iter().map(Credential::from).collect(),
        levels,
        props,
        instances,
    })
}

/// 报告落地：给了 `--out` 就写文件（目录不在就建），否则打到标准输出。
pub fn deliver(report: &Report, out: Option<&Path>) -> anyhow::Result<()> {
    let text = report.render();
    match out {
        Some(path) => {
            if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("创建报告目录失败：{}", dir.display()))?;
            }
            std::fs::write(path, text)
                .with_context(|| format!("写报告失败：{}", path.display()))?;
            eprintln!("[对拍] 报告已写到 {}", path.display());
        }
        None => print!("{text}"),
    }
    eprintln!("[对拍] {}", report.summary_line());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use plant_ui_data::{Attr, AttrKind, EleTreeNode, RefU64};

    use super::*;

    fn node(refno: u64, name: &str) -> EleTreeNode {
        EleTreeNode {
            refno: RefU64::from(refno).into(),
            noun: "ZONE".into(),
            name: name.into(),
            ..Default::default()
        }
    }

    fn attr(name: &str, value: &str) -> Attr {
        Attr {
            name: name.into(),
            value: value.into(),
            kind: AttrKind::Text,
            is_uda: false,
        }
    }

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn parse_args_reads_every_flag_in_both_spellings() {
        let options = parse_args(args(
            "--read-face-parity --depth 3 --sample=10 --roots 24381/2,=24381/3 --out r.md --service http://127.0.0.1:8022/",
        ))
        .unwrap();
        assert_eq!(options.depth, 3);
        assert_eq!(options.sample, 10);
        assert_eq!(
            options.roots,
            vec![
                RefU64::from_two_nums(24381, 2),
                RefU64::from_two_nums(24381, 3)
            ]
        );
        assert_eq!(options.out, Some(PathBuf::from("r.md")));
        assert_eq!(options.service.as_deref(), Some("http://127.0.0.1:8022"));

        let defaults = parse_args(args("--read-face-parity")).unwrap();
        assert_eq!(defaults, Options::default());
        assert_eq!((defaults.depth, defaults.sample), (2, 200));
    }

    #[test]
    fn parse_args_rejects_what_it_does_not_understand() {
        let error = parse_args(args("--read-face-parity --deep 2")).unwrap_err();
        assert!(error.to_string().contains("认不出的参数：--deep"));
        let error = parse_args(args("--read-face-parity --depth")).unwrap_err();
        assert!(error.to_string().contains("--depth 后面要跟一个值"));
        let error = parse_args(args("--read-face-parity --roots abc")).unwrap_err();
        assert!(error.to_string().contains("认不出 refno"));
        let error = parse_args(args("--read-face-parity --sample -1")).unwrap_err();
        assert!(error.to_string().contains("--sample"));
    }

    /// 集合差与原序差分开报：同一批成员换个顺序是「原序不一致」，不是「只在一边」。
    #[test]
    fn diff_children_reports_sets_and_order_separately() {
        let a = node(1, "/A");
        let b = node(2, "/B");
        let c = node(3, "/C");
        let d = node(4, "/D");

        let same = diff_children(&[a.clone(), b.clone()], &[a.clone(), b.clone()]);
        assert_eq!(same.common.len(), 2);
        assert!(same.only_service.is_empty() && same.only_store.is_empty());
        assert!(same.same_order);

        let reordered = diff_children(
            &[a.clone(), b.clone(), c.clone()],
            &[c.clone(), a.clone(), b.clone()],
        );
        assert_eq!(reordered.common.len(), 3);
        assert!(!reordered.same_order);

        let skewed = diff_children(
            &[a.clone(), b.clone(), c.clone()],
            &[a.clone(), c.clone(), d.clone()],
        );
        assert_eq!(
            skewed
                .common
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            ["/A", "/C"]
        );
        assert_eq!(
            skewed
                .only_service
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            ["/B"]
        );
        assert_eq!(
            skewed
                .only_store
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            ["/D"]
        );
        // 共有的 A、C 两边都是 A 在 C 前，原序一致——集合不同不拖累原序的判断。
        assert!(skewed.same_order);
    }

    #[test]
    fn a_level_only_walks_into_nodes_both_sides_have() {
        let mut level = LevelReport::default();
        let next = level.absorb(
            None,
            &[node(1, "/A"), node(2, "/B")],
            &[node(2, "/B"), node(3, "/C")],
        );
        assert_eq!(
            next.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
            ["/B"]
        );
        assert_eq!((level.service, level.store, level.common), (2, 2, 1));
        assert_eq!(level.only_service.len(), 1);
        assert_eq!(level.only_store.len(), 1);
        assert!(level.order_mismatches.is_empty());
        assert!(level.has_differences());
    }

    /// 先比逐字节、再比 trim、再比「去掉全部括号与空白」，剩下的就是不同。
    #[test]
    fn classify_value_follows_the_dialect_ladder() {
        assert_eq!(
            classify_value("ATTRIB PARA[3 ]", "ATTRIB PARA[3 ]"),
            ValueVerdict::Same
        );
        assert_eq!(
            classify_value(" ATTRIB PARA[3 ] ", "ATTRIB PARA[3 ]"),
            ValueVerdict::TrimOnly
        );
        assert_eq!(
            classify_value("( a / b - c / d )", "( ( a / b ) - ( c / d ) )"),
            ValueVerdict::ParenSpaceOnly
        );
        assert_eq!(
            classify_value("( ATTRIB PARA[3 ] )", "ATTRIB PARA[3 ]"),
            ValueVerdict::ParenSpaceOnly
        );
        assert_eq!(classify_value("P(1005)", "-P 5"), ValueVerdict::Different);
        assert_eq!(classify_value("", "( 0 )"), ValueVerdict::Different);
    }

    #[test]
    fn props_compare_files_each_field_and_lists_empty_tables_apart() {
        let mut report = PropsReport::default();
        let z = Node {
            refno: RefU64::from(9),
            noun: "ZONE".into(),
            name: "/Z".into(),
        };
        report.compare(
            &z,
            &[
                attr("NAME", "/Z"),
                attr("PURP", " X "),
                attr("DESC", "( a )"),
                attr("FUNC", "f"),
                attr("ONLYS", "1"),
            ],
            &[
                attr("NAME", "/Z"),
                attr("PURP", "X"),
                attr("DESC", "a"),
                attr("FUNC", "g"),
                attr("ONLYT", "2"),
            ],
        );
        assert_eq!(report.compared, 1);
        assert_eq!(report.field_count(ValueVerdict::Same), 1);
        assert_eq!(report.field_count(ValueVerdict::TrimOnly), 1);
        assert_eq!(report.field_count(ValueVerdict::ParenSpaceOnly), 1);
        assert_eq!(report.field_count(ValueVerdict::Different), 1);
        assert_eq!(report.only_service_fields, 1);
        assert_eq!(report.only_store_fields, 1);
        assert_eq!(report.differences.len(), 1);
        assert_eq!(report.differences[0].field, "FUNC");
        assert_eq!(report.different_by_field.get("FUNC"), Some(&1));
        assert_eq!(report.only_service_by_field.get("ONLYS"), Some(&1));
        assert_eq!(report.only_store_by_field.get("ONLYT"), Some(&1));

        let cata = Node {
            refno: RefU64::from(10),
            noun: "SCOM".into(),
            name: "/C".into(),
        };
        report.compare(&cata, &[attr("NAME", "/C")], &[]);
        assert_eq!(report.store_empty, vec![cata]);
        assert_eq!(report.compared, 1, "空表不进逐字段那一档");
        report.compare(&z, &[], &[]);
        assert_eq!(report.both_empty, 1);
    }

    #[test]
    fn evenly_spaced_covers_the_whole_range_and_never_repeats() {
        assert_eq!(evenly_spaced(0, 5), Vec::<usize>::new());
        assert_eq!(evenly_spaced(3, 0), Vec::<usize>::new());
        assert_eq!(evenly_spaced(3, 5), vec![0, 1, 2]);
        let picks = evenly_spaced(100, 10);
        assert_eq!(picks.len(), 10);
        assert_eq!(picks[0], 0);
        assert!(picks.windows(2).all(|w| w[0] < w[1]));
        assert!(*picks.last().unwrap() >= 90);
    }

    /// 桶按解得出的库号分，解不出的按 refno 高 32 位列出来，不猜。
    #[test]
    fn bucket_instances_groups_by_resolved_dbnum_then_db_ref() {
        let r = |a: u32, b: u32| RefU64::from_two_nums(a, b);
        let mut dbnum_of_ref = HashMap::new();
        dbnum_of_ref.insert(24381, 7997);
        let service = vec![
            (r(24381, 1), "h1".to_owned()),
            (r(24381, 1), "h2".to_owned()),
            (r(16192, 5), "h9".to_owned()),
        ];
        let store = vec![
            (r(24381, 1), "h1".to_owned()),
            (r(16192, 5), "h9".to_owned()),
            (r(16192, 6), String::new()),
        ];
        let buckets = bucket_instances(&service, &store, &dbnum_of_ref);
        assert_eq!(
            buckets.keys().cloned().collect::<Vec<_>>(),
            ["db7997", "ref16192（dbnum 未解）"]
        );
        let desi = &buckets["db7997"];
        assert_eq!(
            (
                desi.service,
                desi.store,
                desi.common,
                desi.only_service,
                desi.only_store
            ),
            (2, 1, 1, 1, 0)
        );
        assert_eq!(desi.samples, vec!["只在服务：=24381/1#h2"]);
        let other = &buckets["ref16192（dbnum 未解）"];
        assert_eq!(
            (
                other.service,
                other.store,
                other.common,
                other.only_service,
                other.only_store
            ),
            (1, 2, 1, 0, 1)
        );
    }

    fn empty_report() -> Report {
        let identity = |project: &str| Identity {
            project: project.into(),
            mdb: "/ALL".into(),
            ns: "1516".into(),
            db_nums: vec![7997, 8000],
            cache_versions: Vec::new(),
        };
        Report {
            generated_at: chrono::Utc::now(),
            elapsed_secs: 1.5,
            options: Options::default(),
            service_base: "http://127.0.0.1:8022".into(),
            store_endpoint: "127.0.0.1:8009 ns 1516 / /ALL".into(),
            data_face: "read-through".into(),
            service_identity: identity("SAM"),
            store_identity: identity("SAM"),
            credentials: vec![Credential {
                dbnum: 7997,
                db_type: "DESI".into(),
                applied_sesno: 41,
                file_latest_sesno: 42,
                excluded: false,
                not_in_project: false,
            }],
            levels: vec![LevelReport {
                parents: 1,
                ..Default::default()
            }],
            props: PropsReport::default(),
            instances: InstanceReport::default(),
        }
    }

    /// 报告只列不判：零差异说「零差异」，有差异只说「有差异」并把两枚凭证摆在前面。
    #[test]
    fn the_report_lists_and_never_judges() {
        let clean = empty_report().render();
        assert!(clean.contains("差异只列不判"));
        assert!(clean.contains("| 7997 | DESI | 41 | 42 | 1 |"));
        assert!(clean.contains("roots 集合与原序**零差异**"));
        assert!(
            !clean.contains("正确") && !clean.contains("错误"),
            "报告里不许出现对错的字眼"
        );

        let mut skewed = empty_report();
        skewed.levels[0].absorb(
            None,
            &[node(1, "/A"), node(2, "/B")],
            &[node(2, "/B"), node(1, "/A")],
        );
        let text = skewed.render();
        assert!(text.contains("roots 集合或原序**有差异**"));
        assert!(text.contains("原序不一致 ← MDB 世界"));
        assert!(text.contains("服务：ZONE /A (=0/1) → ZONE /B (=0/2)"));
        assert!(skewed.summary_line().contains("原序不一致"));
    }

    /// 「不同」与只在一边有的字段都按字段名归堆、次数多的在前：写法差异会整齐落在几个字段上，
    /// 数据差异只落在个别元素上，报告要让人一眼分得出这两种。
    #[test]
    fn the_report_groups_field_differences_by_name() {
        let mut report = empty_report();
        let z = |refno: u64| Node {
            refno: RefU64::from(refno),
            noun: "ZONE".into(),
            name: format!("/Z{refno}"),
        };
        for refno in 1..=3 {
            report.props.compare(
                &z(refno),
                &[attr("REFNO", "1/2"), attr("DESC", "x"), attr("UDA1", "u")],
                &[
                    attr("REFNO", "1_2"),
                    attr("DESC", "x"),
                    attr("LOCK", "false"),
                ],
            );
        }
        report.props.compare(
            &z(4),
            &[attr("REFNO", "1/2"), attr("DESC", "x")],
            &[attr("REFNO", "1_2"), attr("DESC", "y")],
        );
        assert_eq!(
            top_fields(&report.props.different_by_field, 10),
            vec![("REFNO", 4), ("DESC", 1)]
        );
        let text = report.render();
        assert!(text.contains("### 「不同」按字段"));
        assert!(text.contains("| `REFNO` | 4 |"));
        assert!(text.contains("| `DESC` | 1 |"));
        assert!(text.contains("### 只在服务供数有的字段（1 个字段名，按出现的元素数）"));
        assert!(text.contains("| `UDA1` | 3 |"));
        assert!(text.contains("### 只在库供数有的字段（1 个字段名，按出现的元素数）"));
        assert!(text.contains("| `LOCK` | 3 |"));
    }

    /// 两面并存只允许在这个无头子命令里（D2 / D14）：正文恰好造两枚读面，界面宿主一枚都不造
    /// ——宿主那一枚在 `data::spawn` 里，`a_face_is_only_chosen_at_spawn_or_switch` 钉着。
    #[test]
    fn the_probe_is_the_only_place_two_faces_coexist() {
        let body = include_str!("parity.rs")
            .split_once("#[cfg(test)]")
            .map(|(body, _)| body)
            .unwrap();
        assert_eq!(body.matches("ReadFace::new(").count(), 2);
        assert!(!include_str!("../main.rs").contains("ReadFace::new("));
    }
}
