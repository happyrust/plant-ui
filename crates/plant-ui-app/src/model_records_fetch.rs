//! 服务供数取记录的形状（计划 `model-load-speed-parity.md` T1 / T2，照 plant3d-web
//! `src/model-source/genModelV1/modelRecords.ts` 的 `collectRecordsForAncestors`）：
//!
//! - **顶层 SITE 拆成成员。** 调用方（宿主，它手里有 SITE 根层）在 [`ModelRecordsRequest::split_sites`]
//!   里点名哪些目标是顶层 SITE；这些先 `GET /tree/children` 拿成员（ZONE），再对成员取记录。
//!   `tree/children` 失败、或 SITE 下没有成员，退回整根取法——与改前逐字节同一条请求。
//! - **有界并发。** 成员以 [`RECORDS_CONCURRENCY`] 路同时在途，每路取一组、取完再取下一组；
//!   收完一个成员就回报一次进度 `(done, total)`。
//! - **批量形（`generation_roots`）。** 成员多于 [`RECORDS_MEMBER_BATCH_MIN_MEMBERS`] 个、且后端在
//!   `/health.capabilities.model_records_batch_max_roots` 声明了上限时，几个成员打成一发：头几发带
//!   [`RECORDS_BATCH_RAMP`] 个（第一批记录照旧十几毫秒到），之后每发 [`RECORDS_MEMBER_BATCH`] 个；
//!   响应按 `roots[].total` 把平铺的 `items` 切回各成员。整批超过一页时装完的成员照收、没装完的按
//!   条数重新分组再发（**不拿游标翻批量形**——服务端每一页都把整批重读一遍）；单独一根就超过
//!   一页的走单根分页。批量请求没成（400 / 422 / 409 / 传输失败）或响应对不上（没有 `roots`、根名
//!   不符、条数矛盾）时这一组逐个重问、这次读取余下的也不再打包。
//! - **同库不混源不动（ADR-0025）。** 每个成员 / 每发批量的回执都带 `dbnum` / `source` 进
//!   `ModelRecordUnion` 的同源校验；并集按成员的**传入顺序**合，不按回来的先后，结果可复现。
//!
//! 常量与 plant3d-web 一致（计划 Q11），日后两边对照不用换算。环境变量 `PLANT_UI_RECORDS_BATCH`
//! 对应 plant3d-web 页面地址上的 `gm_records_batch`：`0` / `1` 关掉打包，`N` 改稳态每发个数
//! （上限仍是后端声明的、且不超过 [`MAX_MODEL_RECORDS_ROOTS`]）。
//!
//! 传输抽成 [`RecordsTransport`]：真实现 [`HttpRecordsTransport`] 走 `ehttp`；单测用假应答替掉，
//! 逻辑层对此一无所知。这一层仍是**整次成败**：任一成员读失败整次报错让调用点重来（与改前单根
//! 失败的口径相同）；逐成员交付是期 2 的事（T3）。

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::Context as _;
use futures::StreamExt as _;
use futures::stream::FuturesUnordered;
use plant_ui::ModelRecord;
use plant_ui::task_queue::{Health, ModelSource};
use plant_ui_data::{EleTreeNode, RefU64};
use serde::Deserialize;

use super::model_record_union::{ModelRecordUnion, RecordScope};
use super::{ModelRecords, ModelRecordsPage};

/// 同时在途的 `model/records` 数（plant3d-web `concurrency` 缺省 6；后端只读连接池缺省 4 条、
/// 6 个 ZONE 同时在途实测最好，spec §4.5.2）。
pub(crate) const RECORDS_CONCURRENCY: usize = 6;
/// 每页条数，服务端上限（`limit` 夹在 `1..=5000`）。改前就是 5000，不动。
pub(crate) const RECORDS_PAGE_SIZE: usize = 5000;
/// 批量形稳态一发带几个成员（plant3d-web 方案 13.8 节实测：32 个一发不比 16 个快）。
pub(crate) const RECORDS_MEMBER_BATCH: usize = 16;
/// 成员多于这个数才打包：几十个 ZONE 的 SITE 一个一发本来就在一秒以内（19 个 ZONE 两种取法持平）。
pub(crate) const RECORDS_MEMBER_BATCH_MIN_MEMBERS: usize = 64;
/// 服务端一发最多收几个根（spec §4.5.2 `MAX_MODEL_RECORDS_ROOTS`）。
pub(crate) const MAX_MODEL_RECORDS_ROOTS: usize = 64;
/// 批量形头几发各带几个（下标是这次读取里的第几发，排不到的带满）：一上来就带满，第一批记录要等
/// 一整组读完才回来；先发两个只带一个的，第一批照旧十几毫秒就到，边取边画（T3）的第一帧不变晚。
pub(crate) const RECORDS_BATCH_RAMP: [usize; 5] = [1, 1, 2, 4, 8];
/// 单根分页的护栏：异常服务端的游标不许让一次读取无限翻页。
pub(crate) const MAX_MODEL_RECORD_PAGES: usize = 10_000;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
#[cfg(target_arch = "wasm32")]
pub(crate) type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// 发请求的那一步。原生端的模型通道跑在 tokio 上，future 要 `Send`（见 [`BoxFut`]）；
/// 浏览器端单线程，不要。
pub(crate) trait RecordsTransport: Sync {
    /// `POST /api/v1/model/records`：`body` 是请求体 JSON 文本；回执是响应体原文，非 2xx 已包成
    /// `ApiError`。单根与批量两种形状都走它，解析在逻辑层。
    fn post_records<'a>(&'a self, body: String) -> BoxFut<'a, anyhow::Result<String>>;
    /// `GET /api/v1/tree/children?refno=…`：拆顶层 SITE 用。
    fn tree_children<'a>(&'a self, refno: RefU64) -> BoxFut<'a, anyhow::Result<Vec<EleTreeNode>>>;
    /// `GET /api/v1/health`：只为读 `capabilities.model_records_batch_max_roots`。
    fn health<'a>(&'a self) -> BoxFut<'a, anyhow::Result<Health>>;
}

/// 真实现：经 `ehttp` 打模型服务。
pub(crate) struct HttpRecordsTransport {
    base: String,
}

impl HttpRecordsTransport {
    pub(crate) fn new(base: &str) -> Self {
        Self {
            base: base.to_owned(),
        }
    }
}

impl RecordsTransport for HttpRecordsTransport {
    fn post_records<'a>(&'a self, body: String) -> BoxFut<'a, anyhow::Result<String>> {
        Box::pin(super::post_text(
            &self.base,
            "/api/v1/model/records",
            body,
            Duration::from_secs(30),
        ))
    }

    fn tree_children<'a>(&'a self, refno: RefU64) -> BoxFut<'a, anyhow::Result<Vec<EleTreeNode>>> {
        Box::pin(super::tree_children(&self.base, refno))
    }

    fn health<'a>(&'a self) -> BoxFut<'a, anyhow::Result<Health>> {
        Box::pin(super::service_health(&self.base))
    }
}

/// 一次装载要问的祖先与身份。
#[derive(Debug, Clone, Copy)]
pub(crate) struct ModelRecordsRequest<'a> {
    /// 要以之为祖先问 `model/records` 的目标，按序。
    pub scope_roots: &'a [RefU64],
    /// `scope_roots` 里可以拆成成员取的顶层 SITE（调用方从树根层认出来的）。不在这里的目标一个祖先
    /// 一发，请求形状与改前逐字节相同。
    pub split_sites: &'a [RefU64],
    pub project: &'a str,
    pub mdb: &'a str,
    pub namespace: &'a str,
}

/// 取法的旋钮。生产路径由 [`FetchOptions::from_env`] 给（常量 + 环境变量）；单测直接构造。
#[derive(Debug, Clone)]
pub(crate) struct FetchOptions {
    pub concurrency: usize,
    pub page_size: usize,
    /// `PLANT_UI_RECORDS_BATCH` 的值；`None` = 没设。语义见 [`records_member_batch_size`]。
    pub batch_override: Option<String>,
}

impl FetchOptions {
    pub(crate) fn from_env() -> Self {
        Self {
            concurrency: RECORDS_CONCURRENCY,
            page_size: RECORDS_PAGE_SIZE,
            batch_override: records_batch_override().map(str::to_owned),
        }
    }
}

/// `PLANT_UI_RECORDS_BATCH` 只读一次（进程内不会变；浏览器端没有环境变量，恒 `None`）。
fn records_batch_override() -> Option<&'static str> {
    static OVERRIDE: OnceLock<Option<String>> = OnceLock::new();
    OVERRIDE
        .get_or_init(|| {
            std::env::var("PLANT_UI_RECORDS_BATCH")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .as_deref()
}

/// 拆成成员查时一发带几个成员（plant3d-web `recordsMemberBatchSize`）。
///
/// 后端声明了批量形（`declared_max` ≥ 2）、而且成员多于 [`RECORDS_MEMBER_BATCH_MIN_MEMBERS`] 个才打包，
/// 否则 1（一个成员一发；旧后端、`/health` 取不到也是 1）。`override` 是 `PLANT_UI_RECORDS_BATCH`：
/// `0` / `1` 关掉打包，别的非负整数改每发个数（不超过后端声明的上限，也不再看成员数）；乱值不理。
pub(crate) fn records_member_batch_size(
    declared_max: Option<u64>,
    override_value: Option<&str>,
    members: usize,
) -> usize {
    let max = declared_max
        .map(|declared| usize::try_from(declared).unwrap_or(usize::MAX))
        .map(|declared| declared.min(MAX_MODEL_RECORDS_ROOTS))
        .unwrap_or(0);
    if max < 2 {
        return 1;
    }
    if let Some(raw) = override_value.map(str::trim).filter(|raw| !raw.is_empty())
        && let Ok(wanted) = raw.parse::<usize>()
    {
        return wanted.clamp(1, max);
    }
    if members <= RECORDS_MEMBER_BATCH_MIN_MEMBERS {
        return 1;
    }
    RECORDS_MEMBER_BATCH.min(max)
}

/// `PLANT_UI_RECORDS_BATCH=0` / `=1`：明说关掉打包，连 `/health` 都不必问。
fn override_disables_batching(override_value: Option<&str>) -> bool {
    matches!(override_value.map(str::trim), Some("0") | Some("1"))
}

/// 请求体里的身份三件套。
#[derive(Clone, Copy)]
struct Identity<'a> {
    project: &'a str,
    mdb: &'a str,
    namespace: &'a str,
}

/// 单根一页的请求体。**与改前逐字节相同**（键集、`generation_root` 的斜杠形、`limit`、`cursor` 缺省
/// `null`）——非 SITE 目标的请求形状不许因为这次改动变。
fn single_body(
    identity: Identity<'_>,
    root: RefU64,
    limit: usize,
    cursor: Option<usize>,
) -> String {
    serde_json::json!({
        "project": identity.project,
        "mdb": identity.mdb,
        "namespace": identity.namespace,
        "generation_root": root.to_slash_string(),
        "limit": limit,
        "cursor": cursor,
    })
    .to_string()
}

/// 批量形的请求体（spec §4.5.2）：`generation_roots` 斜杠形、不带 `cursor`（批量形从不翻页）。
fn batch_body(identity: Identity<'_>, roots: &[RefU64], limit: usize) -> String {
    serde_json::json!({
        "project": identity.project,
        "mdb": identity.mdb,
        "namespace": identity.namespace,
        "generation_roots": roots.iter().map(RefU64::to_slash_string).collect::<Vec<_>>(),
        "limit": limit,
    })
    .to_string()
}

/// 一个祖先（或批量里的一个成员）收齐的记录与它们的来源。
struct ScopeRecords {
    scope: RecordScope,
    records: Vec<ModelRecord>,
}

/// 取过程里的计数：发了几发 `model/records`、收了多少字节。
#[derive(Default, Clone, Copy)]
struct Tally {
    requests: usize,
    bytes: usize,
}

impl Tally {
    fn note(&mut self, bytes: usize) {
        self.requests += 1;
        self.bytes += bytes;
    }
}

/// 以一个节点为祖先分页读完（改前 `model_records` 的循环体原样）。
async fn fetch_root<T: RecordsTransport + ?Sized>(
    transport: &T,
    identity: Identity<'_>,
    root: RefU64,
    page_size: usize,
    tally: &mut Tally,
) -> anyhow::Result<ScopeRecords> {
    let mut records = Vec::new();
    let mut scope: Option<RecordScope> = None;
    let mut cursor = None;
    for _ in 0..MAX_MODEL_RECORD_PAGES {
        let text = transport
            .post_records(single_body(identity, root, page_size, cursor))
            .await?;
        tally.note(text.len());
        let page: ModelRecordsPage =
            serde_json::from_str(&text).context("解析模型更新服务响应失败")?;
        // 同一根分页中途翻面（第一页内存、第二页 rocksdb）：两页算的未必是同一版，
        // 与跨根混源同一条纪律，整根报错让调用点重来。
        let this = page.scope();
        match scope {
            None => scope = Some(this),
            Some(first) if first != this && this.source.is_some() && first.source.is_some() => {
                anyhow::bail!(
                    "生成根 {} 的记录分页中途换了取数源（{} → {}），这一趟不可用，请重查",
                    root.to_slash_string(),
                    first.source.map(|s| s.as_str()).unwrap_or("?"),
                    this.source.map(|s| s.as_str()).unwrap_or("?"),
                )
            }
            Some(_) => {}
        }
        records.extend(page.items);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None if page.truncated => anyhow::bail!(
                "生成根 {} 的记录分页未完成：truncated=true 但缺少 next_cursor",
                root.to_slash_string()
            ),
            None => {
                return Ok(ScopeRecords {
                    scope: scope.unwrap_or_else(RecordScope::legacy),
                    records,
                });
            }
        }
    }
    anyhow::bail!(
        "生成根 {} 的记录分页达到 {MAX_MODEL_RECORD_PAGES} 页护栏后仍有下一页",
        root.to_slash_string()
    )
}

/// 批量形回执里的逐根总数（`roots[]`）。
#[derive(Debug, Deserialize)]
struct BatchRootTotal {
    #[serde(default)]
    generation_root: String,
    #[serde(default)]
    total: Option<u64>,
}

/// 批量形的一页（spec §4.5.2）。与单根页分开解：`roots` 只有批量形有，gen-model 分组契约快照
/// （`model-records-gpset.json`）是单根形，`ModelRecordsPage` 那张字段清单不该为它多一格。
#[derive(Debug, Deserialize)]
struct ModelRecordsBatchPage {
    #[serde(default)]
    items: Vec<ModelRecord>,
    #[serde(default)]
    roots: Option<Vec<BatchRootTotal>>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    source: String,
    #[serde(default)]
    dbnum: Option<u32>,
}

impl ModelRecordsBatchPage {
    fn scope(&self) -> RecordScope {
        RecordScope {
            dbnum: self.dbnum,
            source: ModelSource::from_records_label(&self.source),
        }
    }
}

/// 批量一页切回各成员之后的样子。
struct SplitBatch {
    /// 这一页里装完的成员的记录，按请求顺序，与 `members[..complete.len()]` 一一对应。
    complete: Vec<Vec<ModelRecord>>,
    /// 排在后面没装完的成员各自的总条数（按请求顺序），由调用方重新分组再发。
    rest_totals: Vec<usize>,
    /// 这一页实际带回的条数（重新分组时一组不超过它）。
    page_rows: usize,
}

/// 把批量形的一页按 `roots[].total`（请求顺序；`items` 也按请求顺序平铺）切回各成员。
///
/// 没截断：每个成员都齐。截断（整批超过一页）：装完的照收，没装完的连同条数回 `rest_totals`。
/// 对不上（没有 `roots`、根数或根名不符、条数不是非负整数、条数和记录数矛盾）回 `None`：调用方
/// 退回一个成员一发，不猜。
fn split_batch_page(page: ModelRecordsBatchPage, members: &[RefU64]) -> Option<SplitBatch> {
    let roots = page.roots.as_ref()?;
    if roots.len() != members.len() {
        return None;
    }
    let mut totals = Vec::with_capacity(roots.len());
    let mut sum = 0usize;
    for (root, member) in roots.iter().zip(members) {
        let total = usize::try_from(root.total?).ok()?;
        if root.generation_root.parse::<RefU64>().ok()? != *member {
            return None;
        }
        totals.push(total);
        sum = sum.checked_add(total)?;
    }
    let page_rows = page.items.len();
    if page.truncated {
        if page_rows >= sum {
            return None;
        }
    } else if page_rows != sum {
        return None;
    }
    let mut items = page.items.into_iter();
    let mut complete = Vec::new();
    let mut offset = 0;
    for &total in &totals {
        if offset + total > page_rows {
            break;
        }
        complete.push(items.by_ref().take(total).collect());
        offset += total;
    }
    let rest_totals = totals[complete.len()..].to_vec();
    Some(SplitBatch {
        complete,
        rest_totals,
        page_rows,
    })
}

/// 并发取成员时各路共用的账。锁只在同步段里拿，从不跨 `await`。
struct Scheduler {
    /// 下一个还没派出去的成员下标。
    next: usize,
    /// 从新成员池里派出的第几组（决定 [`RECORDS_BATCH_RAMP`] 的下标）。
    issued: usize,
    /// 批量响应里一页没装下的成员，按那一回知道的条数重新分好组，排在队头等着再发。
    regrouped: VecDeque<Vec<usize>>,
    /// 还在打包。批量请求没成或响应对不上就翻成 false，余下的一个一发。
    batching: bool,
    /// 有成员读失败：不再派新组，在途的做完就收。
    failed: bool,
    done: usize,
    tally: Tally,
}

struct Shared<'p> {
    sched: Mutex<Scheduler>,
    outcomes: Mutex<Vec<Option<anyhow::Result<ScopeRecords>>>>,
    progress: Mutex<&'p mut (dyn FnMut(usize, usize) + Send)>,
}

impl Shared<'_> {
    fn lock_sched(&self) -> std::sync::MutexGuard<'_, Scheduler> {
        self.sched
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 一个成员有了去向（收齐 / 出错）：记下来，报进度。
    fn settle(&self, index: usize, total: usize, outcome: anyhow::Result<ScopeRecords>) {
        let done = {
            let mut sched = self.lock_sched();
            sched.done += 1;
            if outcome.is_err() {
                sched.failed = true;
            }
            sched.done
        };
        self.outcomes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())[index] = Some(outcome);
        self.report(done, total);
    }

    fn report(&self, done: usize, total: usize) {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (**progress)(done, total);
    }

    fn note_tally(&self, tally: Tally) {
        let mut sched = self.lock_sched();
        sched.tally.requests += tally.requests;
        sched.tally.bytes += tally.bytes;
    }

    /// 下一组要读的成员下标；没有了回 `None`。
    ///
    /// 一组不超过「剩下的平摊到每一路」：服务端一组里是一个接一个读的，一组拿多了，别的几路没活干、
    /// 这一路还在排着读——十几个 ZONE 的 SITE 打成一两个大组反而比一个一发慢。剩得多时（几百个
    /// ZONE）这一条不起作用，照 `batch_max` 带满。
    fn take_group(&self, total: usize, lanes: usize, batch_max: usize) -> Option<Vec<usize>> {
        let mut sched = self.lock_sched();
        if sched.failed {
            return None;
        }
        if let Some(waiting) = sched.regrouped.pop_front() {
            return Some(waiting);
        }
        if sched.next >= total {
            return None;
        }
        let fair_share = (total - sched.next).div_ceil(lanes.max(1));
        let size = if sched.batching {
            let ramp = RECORDS_BATCH_RAMP
                .get(sched.issued)
                .copied()
                .unwrap_or(batch_max);
            batch_max.min(ramp).min(fair_share).max(1)
        } else {
            1
        };
        sched.issued += 1;
        let end = (sched.next + size).min(total);
        let group: Vec<usize> = (sched.next..end).collect();
        sched.next = end;
        Some(group)
    }

    fn batching(&self) -> bool {
        self.lock_sched().batching
    }

    fn stop_batching(&self) {
        self.lock_sched().batching = false;
    }

    fn regroup(&self, groups: Vec<Vec<usize>>) {
        self.lock_sched().regrouped.extend(groups);
    }
}

/// 一路：反复取一组、读一组，直到没有组可取。
async fn lane<T: RecordsTransport + ?Sized>(
    transport: &T,
    identity: Identity<'_>,
    members: &[RefU64],
    page_size: usize,
    lanes: usize,
    batch_max: usize,
    shared: &Shared<'_>,
) {
    let total = members.len();
    while let Some(group) = shared.take_group(total, lanes, batch_max) {
        read_group(transport, identity, members, page_size, group, shared).await;
    }
}

/// 一个成员一发（单根形，分页读完）。
async fn read_single<T: RecordsTransport + ?Sized>(
    transport: &T,
    identity: Identity<'_>,
    members: &[RefU64],
    page_size: usize,
    index: usize,
    shared: &Shared<'_>,
) {
    let mut tally = Tally::default();
    let outcome = fetch_root(transport, identity, members[index], page_size, &mut tally).await;
    shared.note_tally(tally);
    shared.settle(index, members.len(), outcome);
}

async fn read_group<T: RecordsTransport + ?Sized>(
    transport: &T,
    identity: Identity<'_>,
    members: &[RefU64],
    page_size: usize,
    group: Vec<usize>,
    shared: &Shared<'_>,
) {
    if group.len() == 1 || !shared.batching() {
        for index in group {
            read_single(transport, identity, members, page_size, index, shared).await;
            if shared.lock_sched().failed {
                return;
            }
        }
        return;
    }
    let names: Vec<RefU64> = group.iter().map(|&index| members[index]).collect();
    let reply = transport
        .post_records(batch_body(identity, &names, page_size))
        .await;
    let split = match reply {
        Ok(text) => {
            let mut tally = Tally::default();
            tally.note(text.len());
            shared.note_tally(tally);
            serde_json::from_str::<ModelRecordsBatchPage>(&text)
                .ok()
                .and_then(|page| {
                    let scope = page.scope();
                    split_batch_page(page, &names).map(|split| (scope, split))
                })
        }
        Err(_) => {
            // 失败的那一发也算一发（字节 0）。
            shared.note_tally(Tally {
                requests: 1,
                bytes: 0,
            });
            None
        }
    };
    let Some((scope, split)) = split else {
        // 批量请求没成（旧后端不认这个形状、整批里有一根读失败、409 …）或响应对不上：这一组一个一个
        // 重问，这次读取余下的也不再打包——每个成员是空、是出错，由它自己那一发说了算。
        shared.stop_batching();
        for index in group {
            read_single(transport, identity, members, page_size, index, shared).await;
            if shared.lock_sched().failed {
                return;
            }
        }
        return;
    };
    let total = members.len();
    let complete_count = split.complete.len();
    for (k, records) in split.complete.into_iter().enumerate() {
        shared.settle(group[k], total, Ok(ScopeRecords { scope, records }));
    }
    // 一页没装下的：按条数重新分组，每组不超过这一页的实际大小；单独一个就超过一页的自己一组
    // （走单根形分页）。
    let mut regrouped = Vec::new();
    let mut bucket: Vec<usize> = Vec::new();
    let mut bucket_rows = 0usize;
    for (i, &rows) in split.rest_totals.iter().enumerate() {
        let index = group[complete_count + i];
        if rows > split.page_rows {
            if !bucket.is_empty() {
                regrouped.push(std::mem::take(&mut bucket));
                bucket_rows = 0;
            }
            regrouped.push(vec![index]);
            continue;
        }
        if bucket_rows + rows > split.page_rows && !bucket.is_empty() {
            regrouped.push(std::mem::take(&mut bucket));
            bucket_rows = 0;
        }
        bucket.push(index);
        bucket_rows += rows;
    }
    if !bucket.is_empty() {
        regrouped.push(bucket);
    }
    if !regrouped.is_empty() {
        shared.regroup(regrouped);
    }
}

/// 对一批成员（同库、互不包含）各取记录：有界并发、可打包。结果按**传入顺序**回；任一成员失败
/// 整次报错。`progress` 先报 `(0, total)`，之后每收完一个成员报一次。
async fn fetch_members<T: RecordsTransport + ?Sized>(
    transport: &T,
    identity: Identity<'_>,
    members: &[RefU64],
    options: &FetchOptions,
    batch_max: usize,
    progress: &mut (dyn FnMut(usize, usize) + Send),
) -> anyhow::Result<(Vec<ScopeRecords>, Tally)> {
    let total = members.len();
    let lanes = options.concurrency.clamp(1, total.max(1));
    let batch_max = batch_max.clamp(1, MAX_MODEL_RECORDS_ROOTS);
    let shared = Shared {
        sched: Mutex::new(Scheduler {
            next: 0,
            issued: 0,
            regrouped: VecDeque::new(),
            batching: batch_max > 1 && total > 1,
            failed: false,
            done: 0,
            tally: Tally::default(),
        }),
        outcomes: Mutex::new((0..total).map(|_| None).collect()),
        progress: Mutex::new(progress),
    };
    shared.report(0, total);
    {
        let mut running: FuturesUnordered<_> = (0..lanes)
            .map(|_| {
                lane(
                    transport,
                    identity,
                    members,
                    options.page_size,
                    lanes,
                    batch_max,
                    &shared,
                )
            })
            .collect();
        while running.next().await.is_some() {}
    }
    let Shared {
        sched, outcomes, ..
    } = shared;
    let tally = sched
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .tally;
    let outcomes = outcomes
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // 先找失败：有成员失败时别的成员可能还没派出去（`None`），那不是内部错误。
    let mut scopes = Vec::with_capacity(total);
    let mut missing = None;
    for (index, outcome) in outcomes.into_iter().enumerate() {
        match outcome {
            Some(Ok(scope)) => scopes.push(scope),
            Some(Err(error)) => return Err(error),
            None => {
                missing.get_or_insert(index);
            }
        }
    }
    if let Some(index) = missing {
        anyhow::bail!(
            "成员 {} 没有读到记录（内部错误）",
            members[index].to_slash_string()
        );
    }
    Ok((scopes, tally))
}

/// 顶层 SITE 的成员表：`tree/children` 的 refno，去重保序。失败或为空回 `None`，调用方退回整根取法。
async fn members_of<T: RecordsTransport + ?Sized>(
    transport: &T,
    site: RefU64,
) -> Option<Vec<RefU64>> {
    let nodes = transport.tree_children(site).await.ok()?;
    let mut seen = HashSet::new();
    let members: Vec<RefU64> = nodes
        .iter()
        .map(|node| node.refno.refno())
        .filter(|refno| *refno != site && seen.insert(*refno))
        .collect();
    (!members.is_empty()).then_some(members)
}

/// 这次拆成员要不要打包、一发几个。只在可能打包时才问 `/health`（成员够多，或环境变量明说了个数）；
/// 明说关掉、成员不多、`/health` 取不到或没声明 → 1。
async fn decide_batch<T: RecordsTransport + ?Sized>(
    transport: &T,
    members: usize,
    options: &FetchOptions,
) -> usize {
    let override_value = options.batch_override.as_deref();
    if members < 2 || override_disables_batching(override_value) {
        return 1;
    }
    let override_asks_number = override_value
        .map(str::trim)
        .is_some_and(|raw| raw.parse::<usize>().is_ok());
    if members <= RECORDS_MEMBER_BATCH_MIN_MEMBERS && !override_asks_number {
        return 1;
    }
    let declared = match transport.health().await {
        Ok(health) => health.capabilities.model_records_batch_max_roots,
        Err(_) => None,
    };
    records_member_batch_size(declared, override_value, members)
}

/// 一次装载：对 `scope_roots` 逐个取，列在 `split_sites` 的先拆成员；回执按 dbnum 分桶、一桶一源。
pub(crate) async fn collect<T: RecordsTransport + ?Sized>(
    transport: &T,
    req: &ModelRecordsRequest<'_>,
    options: &FetchOptions,
    progress: &mut (dyn FnMut(usize, usize) + Send),
) -> anyhow::Result<ModelRecords> {
    let identity = Identity {
        project: req.project,
        mdb: req.mdb,
        namespace: req.namespace,
    };
    let mut out = ModelRecordUnion::default();
    let mut tally = Tally::default();
    for root in req.scope_roots {
        if req.split_sites.contains(root)
            && let Some(members) = members_of(transport, *root).await
        {
            let batch_max = decide_batch(transport, members.len(), options).await;
            let (scopes, members_tally) =
                fetch_members(transport, identity, &members, options, batch_max, progress).await?;
            for scope in scopes {
                out.extend_scope(scope.scope, scope.records)?;
            }
            tally.requests += members_tally.requests;
            tally.bytes += members_tally.bytes;
            continue;
        }
        let scope = fetch_root(transport, identity, *root, options.page_size, &mut tally).await?;
        out.extend_scope(scope.scope, scope.records)?;
    }
    Ok(ModelRecords {
        sources: out.sources(),
        records: out.finish(),
        requests: tally.requests,
        bytes: tally.bytes,
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use plant_ui::model_update::Failure;

    use crate::model_update_api::ApiError;

    fn run<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("tokio 当前线程运行时")
            .block_on(future)
    }

    fn refno(text: &str) -> RefU64 {
        text.parse().unwrap()
    }

    fn names(count: usize) -> Vec<RefU64> {
        (1..=count).map(|i| refno(&format!("5_{i}"))).collect()
    }

    /// 最小的一条记录（与 `/model/records` 的 `items[]` 同形）。
    fn item(refno: &str, owner: &str) -> serde_json::Value {
        serde_json::json!({
            "refno": refno, "old_refno": null, "owner": owner,
            "world_aabb": {"mins": [0.0, 0.0, 0.0], "maxs": [1.0, 1.0, 1.0]},
            "world_trans": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0]},
            "insts": [], "has_neg": false, "generic": "ELBO", "pts": null, "date": null,
        })
    }

    /// 让出一次再继续：单线程执行器上也能观察到「同时在途」。
    struct YieldOnce(bool);
    impl Future for YieldOnce {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<()> {
            if self.0 {
                std::task::Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        }
    }

    /// 一次请求的记录（测试里按形状断言）。
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Shape {
        Single { root: RefU64, cursor: Option<usize> },
        Batch(Vec<RefU64>),
    }

    impl Shape {
        fn text(&self) -> String {
            match self {
                Shape::Single { root, cursor } => match cursor {
                    Some(cursor) => format!("single:{}@{cursor}", root.to_slash_string()),
                    None => format!("single:{}", root.to_slash_string()),
                },
                Shape::Batch(roots) => roots
                    .iter()
                    .map(RefU64::to_slash_string)
                    .collect::<Vec<_>>()
                    .join("+"),
            }
        }
    }

    type BatchHook = Box<dyn Fn(serde_json::Value) -> serde_json::Value + Send + Sync>;

    /// 假后端：每个成员名下 `rows_for(序号)` 条记录；单根形按游标分页、批量形平铺 + `roots[]`。
    struct Fake {
        rows_for: Box<dyn Fn(u32) -> usize + Send + Sync>,
        calls: Mutex<Vec<Shape>>,
        in_flight: Mutex<(usize, usize)>,
        /// 批量请求的回执经它改一下（造「对不上」）；`None` = 原样。
        batch_hook: Option<BatchHook>,
        /// 批量请求直接回这个错（旧后端 422 / 409）。
        batch_error: Option<Failure>,
        /// 单根请求对这个成员回错。
        single_error: Option<(RefU64, Failure)>,
        children: Mutex<HashMap<RefU64, anyhow::Result<Vec<EleTreeNode>>>>,
        health: Option<Health>,
        /// 这个成员的页标 `source`（造混源）。
        source_for: Box<dyn Fn(RefU64) -> &'static str + Send + Sync>,
    }

    impl Fake {
        fn new(rows_for: impl Fn(u32) -> usize + Send + Sync + 'static) -> Self {
            Self {
                rows_for: Box::new(rows_for),
                calls: Mutex::new(Vec::new()),
                in_flight: Mutex::new((0, 0)),
                batch_hook: None,
                batch_error: None,
                single_error: None,
                children: Mutex::new(HashMap::new()),
                health: Some(health_with(Some(64))),
                source_for: Box::new(|_| "model-database"),
            }
        }

        fn shapes(&self) -> Vec<String> {
            self.calls.lock().unwrap().iter().map(Shape::text).collect()
        }

        fn batch_sizes(&self) -> Vec<usize> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|shape| match shape {
                    Shape::Single { .. } => 1,
                    Shape::Batch(roots) => roots.len(),
                })
                .collect()
        }

        fn rows(&self, member: RefU64) -> Vec<serde_json::Value> {
            let n = member.get_1();
            (0..(self.rows_for)(n))
                .map(|k| {
                    item(
                        &format!("{}_{}", member.get_0() + 1, n * 100 + k as u32),
                        &member.to_string(),
                    )
                })
                .collect()
        }

        fn page(&self, member: RefU64, cursor: usize, limit: usize) -> serde_json::Value {
            let all = self.rows(member);
            let slice: Vec<_> = all.iter().skip(cursor).take(limit).cloned().collect();
            let next = cursor + slice.len();
            let truncated = next < all.len();
            let next_cursor: Option<usize> = truncated.then_some(next);
            serde_json::json!({
                "source": (self.source_for)(member),
                "dbnum": 7997,
                "items": slice,
                "total": all.len(),
                "truncated": truncated,
                "next_cursor": next_cursor,
            })
        }

        fn with_children(self, site: RefU64, members: &[RefU64]) -> Self {
            let nodes = members
                .iter()
                .map(|member| tree_node(*member, site))
                .collect::<Vec<_>>();
            self.children.lock().unwrap().insert(site, Ok(nodes));
            self
        }
    }

    fn tree_node(refno: RefU64, owner: RefU64) -> EleTreeNode {
        serde_json::from_value(serde_json::json!({
            "refno": refno.to_string(), "noun": "ZONE", "name": format!("/Z{}", refno.get_1()),
            "owner": owner.to_string(), "order": 0, "children_count": 3, "op": 0,
            "mod_cnt": null, "children_updated": null, "status_code": null, "dbnum": 7997,
        }))
        .unwrap()
    }

    fn health_with(max_roots: Option<u64>) -> Health {
        let mut health = Health::default();
        health.capabilities.model_records_batch_max_roots = max_roots;
        health
    }

    fn api_error(code: &str, message: &str) -> anyhow::Error {
        anyhow::Error::new(ApiError(Failure::new(code, message)))
    }

    impl RecordsTransport for Fake {
        fn post_records<'a>(&'a self, body: String) -> BoxFut<'a, anyhow::Result<String>> {
            Box::pin(async move {
                {
                    let mut in_flight = self.in_flight.lock().unwrap();
                    in_flight.0 += 1;
                    in_flight.1 = in_flight.1.max(in_flight.0);
                }
                YieldOnce(false).await;
                let result = self.answer(&body);
                self.in_flight.lock().unwrap().0 -= 1;
                result
            })
        }

        fn tree_children<'a>(
            &'a self,
            refno: RefU64,
        ) -> BoxFut<'a, anyhow::Result<Vec<EleTreeNode>>> {
            Box::pin(async move {
                match self.children.lock().unwrap().remove(&refno) {
                    Some(Ok(nodes)) => Ok(nodes),
                    Some(Err(error)) => Err(error),
                    None => Err(anyhow::anyhow!("tree/children 没有这个节点")),
                }
            })
        }

        fn health<'a>(&'a self) -> BoxFut<'a, anyhow::Result<Health>> {
            Box::pin(async move {
                self.health
                    .clone()
                    .ok_or_else(|| api_error("timeout", "health 取不到"))
            })
        }
    }

    impl Fake {
        fn answer(&self, body: &str) -> anyhow::Result<String> {
            let body: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(body["project"], "SAM");
            assert_eq!(body["mdb"], "/MDB");
            assert_eq!(body["namespace"], "plant");
            let limit = body["limit"].as_u64().unwrap() as usize;
            if let Some(roots) = body["generation_roots"].as_array() {
                assert!(body.get("cursor").is_none(), "批量形从不带游标");
                assert!(body.get("generation_root").is_none(), "两种形状互斥");
                let roots: Vec<RefU64> = roots
                    .iter()
                    .map(|root| refno(root.as_str().unwrap()))
                    .collect();
                self.calls.lock().unwrap().push(Shape::Batch(roots.clone()));
                if let Some(failure) = &self.batch_error {
                    return Err(anyhow::Error::new(ApiError(failure.clone())));
                }
                let all: Vec<serde_json::Value> =
                    roots.iter().flat_map(|root| self.rows(*root)).collect();
                let slice: Vec<_> = all.iter().take(limit).cloned().collect();
                let truncated = all.len() > limit;
                let next_cursor: Option<usize> = truncated.then_some(limit);
                let generation_roots: Vec<String> =
                    roots.iter().map(RefU64::to_slash_string).collect();
                let root_totals: Vec<serde_json::Value> = roots
                    .iter()
                    .map(|root| {
                        serde_json::json!({
                            "generation_root": root.to_slash_string(),
                            "total": self.rows(*root).len(),
                        })
                    })
                    .collect();
                let mut reply = serde_json::json!({
                    "source": "model-database",
                    "dbnum": 7997,
                    "generation_roots": generation_roots,
                    "roots": root_totals,
                    "items": slice,
                    "total": all.len(),
                    "truncated": truncated,
                    "next_cursor": next_cursor,
                });
                if let Some(hook) = &self.batch_hook {
                    reply = hook(reply);
                }
                return Ok(reply.to_string());
            }
            let root = refno(body["generation_root"].as_str().unwrap());
            let cursor = body["cursor"].as_u64().map(|c| c as usize);
            self.calls
                .lock()
                .unwrap()
                .push(Shape::Single { root, cursor });
            if let Some((bad, failure)) = &self.single_error
                && *bad == root
            {
                return Err(anyhow::Error::new(ApiError(failure.clone())));
            }
            Ok(self.page(root, cursor.unwrap_or(0), limit).to_string())
        }
    }

    fn options(concurrency: usize, page_size: usize, batch_override: Option<&str>) -> FetchOptions {
        FetchOptions {
            concurrency,
            page_size,
            batch_override: batch_override.map(str::to_owned),
        }
    }

    const SITE: &str = "5_0";

    fn request<'a>(
        scope_roots: &'a [RefU64],
        split_sites: &'a [RefU64],
    ) -> ModelRecordsRequest<'a> {
        ModelRecordsRequest {
            scope_roots,
            split_sites,
            project: "SAM",
            mdb: "/MDB",
            namespace: "plant",
        }
    }

    fn collect_with(
        fake: &Fake,
        scope_roots: &[RefU64],
        split_sites: &[RefU64],
        options: &FetchOptions,
    ) -> (anyhow::Result<ModelRecords>, Vec<(usize, usize)>) {
        let mut progress = Vec::new();
        let result = run(collect(
            fake,
            &request(scope_roots, split_sites),
            options,
            &mut |done, total| progress.push((done, total)),
        ));
        (result, progress)
    }

    fn refnos(records: &ModelRecords) -> Vec<String> {
        records
            .records
            .iter()
            .map(|record| record.refno.refno().to_string())
            .collect()
    }

    // ---- T1：非 SITE 目标 / 退回整根 / 拆成员 ----

    /// 非 SITE 目标（不在 `split_sites`）：一个祖先一发、按游标翻完，请求体与改前逐字节相同
    /// （键集、斜杠形 refno、`limit: 5000`、首页 `cursor: null`）。
    #[test]
    fn a_plain_target_is_one_root_one_request_with_the_old_body() {
        let zone = refno("5_7");
        let fake = Fake::new(|_| 3);
        let (result, progress) = collect_with(&fake, &[zone], &[], &options(6, 5000, None));
        let records = result.unwrap();
        assert_eq!(fake.shapes(), vec!["single:5/7"]);
        assert_eq!(records.records.len(), 3);
        assert_eq!(records.requests, 1);
        assert!(records.bytes > 0);
        assert!(progress.is_empty(), "整根取法不报进度（与改前相同）");
        assert_eq!(
            single_body(
                Identity {
                    project: "SAM",
                    mdb: "/MDB",
                    namespace: "plant"
                },
                zone,
                5000,
                None
            ),
            serde_json::json!({
                "project": "SAM", "mdb": "/MDB", "namespace": "plant",
                "generation_root": "5/7", "limit": 5000, "cursor": null,
            })
            .to_string()
        );
    }

    /// 分页：翻完为止；`truncated: true` 却没有 `next_cursor` 是矛盾回执，整根失败。
    #[test]
    fn a_single_root_pages_to_the_end_and_fails_when_truncated_without_a_cursor() {
        let zone = refno("5_1");
        let fake = Fake::new(|_| 25);
        let (result, _) = collect_with(&fake, &[zone], &[], &options(6, 10, None));
        assert_eq!(refnos(&result.unwrap()).len(), 25);
        assert_eq!(
            fake.shapes(),
            vec!["single:5/1", "single:5/1@10", "single:5/1@20"]
        );

        let text = serde_json::json!({
            "source": "model-database", "dbnum": 7997, "items": [], "total": 3,
            "truncated": true, "next_cursor": null,
        })
        .to_string();
        let page: ModelRecordsPage = serde_json::from_str(&text).unwrap();
        assert!(page.truncated && page.next_cursor.is_none());
        struct Contradicting(String);
        impl RecordsTransport for Contradicting {
            fn post_records<'a>(&'a self, _: String) -> BoxFut<'a, anyhow::Result<String>> {
                Box::pin(async move { Ok(self.0.clone()) })
            }
            fn tree_children<'a>(
                &'a self,
                _: RefU64,
            ) -> BoxFut<'a, anyhow::Result<Vec<EleTreeNode>>> {
                Box::pin(async { Err(anyhow::anyhow!("不该问")) })
            }
            fn health<'a>(&'a self) -> BoxFut<'a, anyhow::Result<Health>> {
                Box::pin(async { Err(anyhow::anyhow!("不该问")) })
            }
        }
        let error = run(collect(
            &Contradicting(text),
            &request(&[zone], &[]),
            &options(6, 5000, None),
            &mut |_, _| {},
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("分页未完成"), "{error}");
    }

    /// 顶层 SITE：先 `tree/children` 拿成员，成员各自一发；记录按成员顺序拼接，请求数 = 成员数；
    /// 进度先 `(0, n)` 再逐个 `(k, n)`。
    #[test]
    fn a_top_site_is_split_into_its_members() {
        let site = refno(SITE);
        let members = names(5);
        let fake =
            Fake::new(|n| if n % 3 == 0 { 0 } else { n as usize }).with_children(site, &members);
        let (result, progress) = collect_with(&fake, &[site], &[site], &options(6, 5000, None));
        let records = result.unwrap();
        let mut shapes = fake.shapes();
        shapes.sort();
        assert_eq!(
            shapes,
            vec![
                "single:5/1",
                "single:5/2",
                "single:5/3",
                "single:5/4",
                "single:5/5"
            ]
        );
        assert_eq!(records.requests, 5);
        // 1 + 2 + 0 + 4 + 5 条，按成员顺序
        assert_eq!(records.records.len(), 12);
        let owners: Vec<String> = records
            .records
            .iter()
            .map(|record| record.owner.refno().to_string())
            .collect();
        let mut sorted = owners.clone();
        sorted.sort_by_key(|owner| owner.split('_').nth(1).unwrap().parse::<u32>().unwrap());
        assert_eq!(owners, sorted, "并集按成员的传入顺序合，不按回来的先后");
        assert_eq!(progress.first(), Some(&(0, 5)));
        assert_eq!(progress.last(), Some(&(5, 5)));
        assert_eq!(progress.len(), 6);
        assert_eq!(
            records.sources,
            vec![(Some(7997), ModelSource::Database)],
            "每个成员的 dbnum / source 都进了同源校验"
        );
    }

    /// 在途单根请求最多 `concurrency` 个：7 个以上成员时第 7 个要等前面某个回来。
    #[test]
    fn members_are_fetched_with_bounded_concurrency() {
        let site = refno(SITE);
        let members = names(40);
        let fake = Fake::new(|_| 1).with_children(site, &members);
        let (result, _) = collect_with(&fake, &[site], &[site], &options(6, 5000, None));
        assert_eq!(result.unwrap().records.len(), 40);
        assert_eq!(fake.in_flight.lock().unwrap().1, 6, "同时在途最多 6 个");
        assert_eq!(fake.calls.lock().unwrap().len(), 40);
    }

    /// 同库不混源：一个 ZONE 回 `model-memory`、其余回 `model-database`，整次按 ADR-0025 报错重来。
    #[test]
    fn a_member_from_another_source_fails_the_whole_load() {
        let site = refno(SITE);
        let members = names(4);
        let mut fake = Fake::new(|_| 2).with_children(site, &members);
        fake.source_for = Box::new(|member| {
            if member.get_1() == 3 {
                "model-memory"
            } else {
                "model-database"
            }
        });
        let (result, _) = collect_with(&fake, &[site], &[site], &options(6, 5000, None));
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("db7997") && error.contains("混了两个取数源"),
            "{error}"
        );
    }

    /// 一个成员读失败：整次失败（与改前单根失败同一口径），不再派新组。
    #[test]
    fn a_failing_member_fails_the_whole_load_and_stops_new_requests() {
        let site = refno(SITE);
        let members = names(30);
        let mut fake = Fake::new(|_| 1).with_children(site, &members);
        fake.single_error = Some((refno("5_2"), Failure::new("internal", "boom")));
        let (result, _) = collect_with(&fake, &[site], &[site], &options(6, 5000, None));
        let error = result.unwrap_err().to_string();
        assert!(error.contains("boom"), "{error}");
        assert!(
            fake.calls.lock().unwrap().len() < 30,
            "失败之后不再把余下的成员派出去"
        );
    }

    /// `tree/children` 失败、SITE 没有成员：退回整根取法，与非 SITE 目标同一条请求、同一结果。
    #[test]
    fn a_site_without_members_or_without_children_falls_back_to_the_whole_root() {
        let site = refno(SITE);
        let fake = Fake::new(|_| 4).with_children(site, &[]);
        let (fallback, progress) = collect_with(&fake, &[site], &[site], &options(6, 5000, None));
        let fallback = fallback.unwrap();
        assert_eq!(fake.shapes(), vec!["single:5/0"]);
        assert!(progress.is_empty());

        let failing = Fake::new(|_| 4);
        failing
            .children
            .lock()
            .unwrap()
            .insert(site, Err(anyhow::anyhow!("tree 503")));
        let (failed, _) = collect_with(&failing, &[site], &[site], &options(6, 5000, None));
        let failed = failed.unwrap();
        assert_eq!(failing.shapes(), vec!["single:5/0"]);

        let plain = Fake::new(|_| 4);
        let (plain_result, _) = collect_with(&plain, &[site], &[], &options(6, 5000, None));
        let plain_result = plain_result.unwrap();
        let json = |records: &ModelRecords| serde_json::to_string(&records.records).unwrap();
        assert_eq!(
            json(&fallback),
            json(&plain_result),
            "退回整根与非 SITE 目标逐字节同结果"
        );
        assert_eq!(json(&failed), json(&plain_result));
        assert_eq!(fallback.sources, plain_result.sources);
    }

    // ---- T2：批量形 ----

    /// 没有 `split_sites` 里的目标、或成员不多（≤ 64）：一个批量形请求都不发，与 T1 相同。
    #[test]
    fn few_members_are_never_batched() {
        let site = refno(SITE);
        let members = names(19);
        let fake = Fake::new(|_| 1).with_children(site, &members);
        let (result, _) = collect_with(&fake, &[site], &[site], &options(6, 5000, None));
        assert_eq!(result.unwrap().records.len(), 19);
        assert!(
            fake.shapes()
                .iter()
                .all(|shape| shape.starts_with("single:"))
        );
        let members = names(RECORDS_MEMBER_BATCH_MIN_MEMBERS);
        let fake = Fake::new(|_| 1).with_children(site, &members);
        let (result, _) = collect_with(&fake, &[site], &[site], &options(6, 5000, None));
        assert_eq!(result.unwrap().records.len(), 64);
        assert!(
            fake.shapes()
                .iter()
                .all(|shape| shape.starts_with("single:"))
        );
    }

    /// 成员多于 64 个且后端声明了批量形：头几发 1、1、2、4、8，之后带满 16；结果与一个一个查相同。
    #[test]
    fn many_members_ramp_up_then_batch_sixteen_and_match_the_plain_result() {
        let site = refno(SITE);
        let members = names(100);
        let rows = |n: u32| if n % 3 == 0 { 0 } else { (n % 5) as usize + 1 };
        let fake = Fake::new(rows).with_children(site, &members);
        let (result, progress) = collect_with(&fake, &[site], &[site], &options(1, 5000, None));
        let batched = result.unwrap();
        // 1 路：1、1、2、4、8，然后 16、16、16、16、16，剩 4
        assert_eq!(
            fake.batch_sizes(),
            vec![1, 1, 2, 4, 8, 16, 16, 16, 16, 16, 4]
        );
        assert_eq!(batched.requests, 11);
        assert_eq!(progress.len(), 101);
        assert_eq!(progress.last(), Some(&(100, 100)));

        let plain = Fake::new(rows).with_children(site, &members);
        let (plain_result, _) =
            collect_with(&plain, &[site], &[site], &options(1, 5000, Some("0")));
        let plain_result = plain_result.unwrap();
        assert!(
            plain
                .shapes()
                .iter()
                .all(|shape| shape.starts_with("single:"))
        );
        assert_eq!(
            serde_json::to_string(&batched.records).unwrap(),
            serde_json::to_string(&plain_result.records).unwrap(),
            "打包与不打包逐字节同结果"
        );
        assert_eq!(batched.sources, plain_result.sources);
    }

    /// 并发时各路各取下一组、同时在途不超过 `concurrency`；一组不超过「剩下的平摊到每一路」。
    #[test]
    fn batched_lanes_share_the_tail_fairly() {
        let site = refno(SITE);
        let members = names(100);
        let fake = Fake::new(|_| 1).with_children(site, &members);
        let (result, _) = collect_with(&fake, &[site], &[site], &options(3, 5000, Some("8")));
        assert_eq!(result.unwrap().records.len(), 100);
        assert!(fake.in_flight.lock().unwrap().1 <= 3);
        let sizes = fake.batch_sizes();
        assert_eq!(&sizes[..5], &[1, 1, 2, 4, 8]);
        assert!(sizes.iter().all(|size| *size <= 8));
        assert_eq!(sizes.iter().sum::<usize>(), 100);
        // 收尾那几发越来越小：剩得少时一组不超过剩下的平摊到 3 路
        assert!(sizes.last().is_some_and(|last| *last <= 3), "{sizes:?}");
    }

    /// 一页装不下：装完的照收，没装完的按条数重新分组再发（不拿游标翻批量形）；单独就超过一页的走
    /// 单根形分页。
    #[test]
    fn an_overflowing_batch_page_regroups_the_rest_by_row_count() {
        let site = refno(SITE);
        let members = names(70);
        // 页大小 10。5_3..5_6 是 4 + 9 + 25 + 3 条：第一页只装下 5_3（5_4 只来了一部分，不要）
        let rows = |n: u32| match n {
            1 | 2 => 1,
            3 => 4,
            4 => 9,
            5 => 25,
            6 => 3,
            _ => 0,
        };
        let fake = Fake::new(rows).with_children(site, &members);
        let (result, _) = collect_with(&fake, &[site], &[site], &options(1, 10, None));
        let records = result.unwrap();
        let shapes = fake.shapes();
        assert_eq!(
            &shapes[..10],
            &[
                "single:5/1",
                "single:5/2",
                "5/3+5/4",         // 4 + 9 > 10：截断，5_3 齐了
                "single:5/4",      // 9 条，自己一组
                "5/5+5/6+5/7+5/8", // 25 + 3 + 0 + 0：截断，一个都没装完
                "single:5/5",
                "single:5/5@10",
                "single:5/5@20", // 25 条按游标翻三页
                "5/6+5/7+5/8",   // 3 + 0 + 0 条，合成一组再发，正好装下
                "5/9+5/10+5/11+5/12+5/13+5/14+5/15+5/16",
            ]
        );
        assert_eq!(records.records.len(), 1 + 1 + 4 + 9 + 25 + 3);
        let expected: Vec<String> = members
            .iter()
            .flat_map(|member| fake.rows(*member))
            .map(|item| item["refno"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(refnos(&records), expected, "记录仍按成员顺序");
    }

    /// 一页装不下、没装完的几个加起来不超过一页：合成一组再发一次。
    #[test]
    fn leftovers_that_fit_one_page_are_resent_together() {
        let site = refno(SITE);
        let members = names(65);
        let rows = |n: u32| match n {
            1 | 2 => 1,
            3 | 4 => 2,
            5 => 8,
            6..=8 => 3,
            _ => 0,
        };
        let fake = Fake::new(rows).with_children(site, &members);
        let (result, _) = collect_with(&fake, &[site], &[site], &options(1, 10, None));
        assert_eq!(result.unwrap().records.len(), 1 + 1 + 2 + 2 + 8 + 9);
        let shapes = fake.shapes();
        // 5_5..5_8 = 8 + 3 + 3 + 3 = 17 条：第一页装完 5_5，5_6 只来了 2 条 → 5_6+5_7+5_8（9 条）再发一次
        assert_eq!(
            &shapes[..5],
            &[
                "single:5/1",
                "single:5/2",
                "5/3+5/4",
                "5/5+5/6+5/7+5/8",
                "5/6+5/7+5/8"
            ]
        );
    }

    /// 批量请求没成（旧后端 422）：这一组一个一个重问，余下的也不再打包；结果不受影响。
    #[test]
    fn a_rejected_batch_falls_back_to_singles_for_the_rest_of_the_load() {
        let site = refno(SITE);
        let members = names(70);
        let mut fake = Fake::new(|_| 2).with_children(site, &members);
        fake.batch_error = Some(Failure::new("precondition", "unknown field"));
        let (result, _) = collect_with(&fake, &[site], &[site], &options(1, 5000, None));
        let records = result.unwrap();
        assert_eq!(records.records.len(), 140);
        let shapes = fake.shapes();
        assert_eq!(
            &shapes[..5],
            &[
                "single:5/1",
                "single:5/2",
                "5/3+5/4",
                "single:5/3",
                "single:5/4"
            ]
        );
        assert!(shapes[5..].iter().all(|shape| shape.starts_with("single:")));
        assert_eq!(shapes.len(), 71, "一发批量 + 70 发单根");
        assert_eq!(records.requests, 71);
    }

    /// 整批回 409（库正被整库任务占用）：逐个重问之后，单根也回 409 → 整次失败、不猜成查空。
    #[test]
    fn a_busy_batch_is_retried_singly_and_the_single_verdict_stands() {
        let site = refno(SITE);
        let members = names(70);
        let mut fake = Fake::new(|_| 2).with_children(site, &members);
        fake.batch_error = Some(Failure::new("conflict", "busy"));
        fake.single_error = Some((refno("5_3"), Failure::new("conflict", "busy")));
        let (result, _) = collect_with(&fake, &[site], &[site], &options(1, 5000, None));
        let error = result.unwrap_err().to_string();
        assert!(error.contains("busy"), "{error}");
        assert_eq!(
            &fake.shapes()[..4],
            &["single:5/1", "single:5/2", "5/3+5/4", "single:5/3"]
        );
    }

    /// 响应对不上（没有 `roots`、根名不符、条数矛盾、根数不符、截断却条数齐）：不猜，逐个重问。
    #[test]
    fn a_mismatched_batch_reply_is_not_trusted() {
        let site = refno(SITE);
        let members = names(66);
        let hooks: Vec<BatchHook> = vec![
            Box::new(|mut reply| {
                reply.as_object_mut().unwrap().remove("roots");
                reply
            }),
            Box::new(|mut reply| {
                reply["roots"][0]["generation_root"] = serde_json::json!("9/9");
                reply
            }),
            Box::new(|mut reply| {
                let total = reply["roots"][0]["total"].as_u64().unwrap();
                reply["roots"][0]["total"] = serde_json::json!(total + 1);
                reply
            }),
            Box::new(|mut reply| {
                reply["roots"].as_array_mut().unwrap().remove(0);
                reply
            }),
            Box::new(|mut reply| {
                reply["truncated"] = serde_json::json!(true);
                reply["next_cursor"] = serde_json::json!(1);
                reply
            }),
            Box::new(|mut reply| {
                reply["roots"][0]["total"] = serde_json::json!("2");
                reply
            }),
        ];
        for hook in hooks {
            let mut fake = Fake::new(|_| 2).with_children(site, &members);
            fake.batch_hook = Some(hook);
            let (result, _) = collect_with(&fake, &[site], &[site], &options(1, 5000, None));
            let records = result.unwrap();
            assert_eq!(records.records.len(), 132);
            let shapes = fake.shapes();
            assert_eq!(
                &shapes[..5],
                &[
                    "single:5/1",
                    "single:5/2",
                    "5/3+5/4",
                    "single:5/3",
                    "single:5/4"
                ]
            );
            assert!(shapes[5..].iter().all(|shape| shape.starts_with("single:")));
            let expected: Vec<String> = members
                .iter()
                .flat_map(|member| fake.rows(*member))
                .map(|item| item["refno"].as_str().unwrap().to_owned())
                .collect();
            assert_eq!(refnos(&records), expected);
        }
    }

    /// 一发最多带 64 个（服务端上限），环境变量给得再大也不超；后端声明得更小就按后端的。
    #[test]
    fn a_batch_never_exceeds_the_server_limit() {
        let site = refno(SITE);
        let members = names(200);
        let mut fake = Fake::new(|_| 0).with_children(site, &members);
        fake.health = Some(health_with(Some(1000)));
        let (result, _) = collect_with(&fake, &[site], &[site], &options(1, 5000, Some("500")));
        result.unwrap();
        assert_eq!(fake.batch_sizes().into_iter().max(), Some(64));

        let mut fake = Fake::new(|_| 0).with_children(site, &members);
        fake.health = Some(health_with(Some(8)));
        let (result, _) = collect_with(&fake, &[site], &[site], &options(1, 5000, Some("500")));
        result.unwrap();
        assert_eq!(fake.batch_sizes().into_iter().max(), Some(8));
    }

    /// `/health` 没声明批量形、或 `/health` 取不到：不打包；成员 ≤ 64 时连 `/health` 都不问。
    #[test]
    fn without_a_declared_capability_nothing_is_batched() {
        let site = refno(SITE);
        let members = names(70);
        for health in [None, Some(health_with(None)), Some(health_with(Some(1)))] {
            let mut fake = Fake::new(|_| 1).with_children(site, &members);
            fake.health = health;
            let (result, _) = collect_with(&fake, &[site], &[site], &options(6, 5000, None));
            assert_eq!(result.unwrap().records.len(), 70);
            assert!(
                fake.shapes()
                    .iter()
                    .all(|shape| shape.starts_with("single:"))
            );
        }
    }

    /// `PLANT_UI_RECORDS_BATCH`：`0` / `1` 关掉（请求形状与 T1 逐字节相同）；`32` 稳态每发 32 个；
    /// 没设时 1、1、2、4、8、16…；乱值不理（按没设）。
    #[test]
    fn the_env_override_turns_batching_off_or_sets_the_batch_size() {
        let site = refno(SITE);
        let members = names(100);
        for off in ["0", "1", " 0 "] {
            let fake = Fake::new(|_| 1).with_children(site, &members);
            let (result, _) = collect_with(&fake, &[site], &[site], &options(1, 5000, Some(off)));
            assert_eq!(result.unwrap().records.len(), 100);
            let shapes = fake.shapes();
            assert!(
                shapes.iter().all(|shape| shape.starts_with("single:")),
                "{off:?}"
            );
            assert_eq!(shapes.len(), 100);
        }
        let fake = Fake::new(|_| 1).with_children(site, &members);
        let (result, _) = collect_with(&fake, &[site], &[site], &options(1, 5000, Some("32")));
        assert_eq!(result.unwrap().records.len(), 100);
        assert_eq!(fake.batch_sizes(), vec![1, 1, 2, 4, 8, 32, 32, 20]);

        let fake = Fake::new(|_| 1).with_children(site, &members);
        let (result, _) = collect_with(&fake, &[site], &[site], &options(1, 5000, None));
        assert_eq!(result.unwrap().records.len(), 100);
        assert_eq!(
            fake.batch_sizes(),
            vec![1, 1, 2, 4, 8, 16, 16, 16, 16, 16, 4]
        );

        let fake = Fake::new(|_| 1).with_children(site, &members);
        let (result, _) = collect_with(&fake, &[site], &[site], &options(1, 5000, Some("abc")));
        assert_eq!(result.unwrap().records.len(), 100);
        assert_eq!(
            fake.batch_sizes(),
            vec![1, 1, 2, 4, 8, 16, 16, 16, 16, 16, 4]
        );
    }

    /// 环境变量明说了个数：成员不多也打包（不再看成员数），与 plant3d-web `gm_records_batch` 同口径。
    #[test]
    fn an_explicit_batch_size_applies_even_to_few_members() {
        let site = refno(SITE);
        let members = names(19);
        let fake = Fake::new(|_| 1).with_children(site, &members);
        let (result, _) = collect_with(&fake, &[site], &[site], &options(1, 5000, Some("8")));
        assert_eq!(result.unwrap().records.len(), 19);
        assert_eq!(fake.batch_sizes(), vec![1, 1, 2, 4, 8, 3]);
    }

    /// `recordsMemberBatchSize` 的纯函数口径（plant3d-web 同名测试）。
    #[test]
    fn member_batch_size_follows_the_capability_the_override_and_the_member_count() {
        assert_eq!(records_member_batch_size(None, None, 531), 1);
        assert_eq!(records_member_batch_size(Some(1), None, 531), 1);
        assert_eq!(
            records_member_batch_size(Some(64), None, 531),
            RECORDS_MEMBER_BATCH
        );
        assert_eq!(records_member_batch_size(Some(8), None, 531), 8);
        assert_eq!(
            records_member_batch_size(Some(1000), None, 531),
            RECORDS_MEMBER_BATCH
        );
        assert_eq!(records_member_batch_size(Some(64), None, 19), 1);
        assert_eq!(records_member_batch_size(Some(64), None, 64), 1);
        assert_eq!(
            records_member_batch_size(Some(64), None, 65),
            RECORDS_MEMBER_BATCH
        );
        assert_eq!(records_member_batch_size(Some(64), Some("8"), 19), 8);
        assert_eq!(records_member_batch_size(Some(64), Some("0"), 531), 1);
        assert_eq!(records_member_batch_size(Some(64), Some("1"), 531), 1);
        assert_eq!(records_member_batch_size(Some(64), Some("500"), 531), 64);
        assert_eq!(records_member_batch_size(Some(8), Some("500"), 531), 8);
        assert_eq!(
            records_member_batch_size(Some(64), Some("abc"), 531),
            RECORDS_MEMBER_BATCH
        );
        assert_eq!(
            records_member_batch_size(Some(64), Some(""), 531),
            RECORDS_MEMBER_BATCH
        );
        assert!(override_disables_batching(Some("0")));
        assert!(override_disables_batching(Some("1")));
        assert!(!override_disables_batching(Some("2")));
        assert!(!override_disables_batching(None));
    }

    /// 批量页切回成员的纯函数：齐 / 截断 / 几种对不上。
    #[test]
    fn splitting_a_batch_page_is_strict() {
        let members = names(3);
        let page = |items: Vec<serde_json::Value>, totals: [u64; 3], truncated: bool| {
            serde_json::from_value::<ModelRecordsBatchPage>(serde_json::json!({
                "source": "model-database", "dbnum": 7997, "items": items, "truncated": truncated,
                "roots": members.iter().zip(totals).map(|(m, t)| serde_json::json!({
                    "generation_root": m.to_slash_string(), "total": t,
                })).collect::<Vec<_>>(),
            }))
            .unwrap()
        };
        let rows = |member: &RefU64, n: usize| -> Vec<serde_json::Value> {
            (0..n)
                .map(|k| item(&format!("6_{}{k}", member.get_1()), &member.to_string()))
                .collect()
        };
        let all: Vec<_> = rows(&members[0], 2)
            .into_iter()
            .chain(rows(&members[1], 0))
            .chain(rows(&members[2], 3))
            .collect();
        let split = split_batch_page(page(all.clone(), [2, 0, 3], false), &members).unwrap();
        assert_eq!(
            split.complete.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![2, 0, 3]
        );
        assert!(split.rest_totals.is_empty());
        // 截断：只来了前 4 条，第三个成员没装完
        let split = split_batch_page(page(all[..4].to_vec(), [2, 0, 3], true), &members).unwrap();
        assert_eq!(
            split.complete.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![2, 0]
        );
        assert_eq!(split.rest_totals, vec![3]);
        assert_eq!(split.page_rows, 4);
        // 对不上：条数矛盾 / 截断却条数齐 / 根数不符
        assert!(split_batch_page(page(all.clone(), [2, 1, 3], false), &members).is_none());
        assert!(split_batch_page(page(all.clone(), [2, 0, 3], true), &members).is_none());
        assert!(split_batch_page(page(all, [2, 0, 3], false), &members[..2]).is_none());
    }
}
