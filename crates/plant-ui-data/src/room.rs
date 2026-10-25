//! 房间查询：`room_relate` / `room_panel_relate` -> 与绘制无关的纯数据。
//!
//! 口径与 gen-model ADR-010 §5 一致：归属强度 = `inside_count` 降序 ->
//! `center_dist` 升序 -> `room_num` 升序。容器聚合、命名校验等六条取向见
//! `docs/plans/room-info-feature.md` 第四节。
//!
//! 归属边挂在**叶子元素**上（`fn::room_code` 开头那段 EQUI/BRAN 归口是死代码，
//! 别被它迷惑）；容器元素照 `fn::get_room_names` 的模式聚合**直接子层**
//! （`pe_owner` 边：in = 子，out = 父）。再深一层的几何件（EQUI -> SUBE -> 件）
//! 一期不追，追层数是数据侧口径问题，先与 `fn::get_room_names` 保持一致。

use std::collections::{HashMap, HashSet};

use aios_core::{RefU64, RefnoEnum, SUL_DB};
use anyhow::Result;

/// 元素对一间房的一条归属（同房多面板已归并成最强一条）。
#[derive(Debug, Clone, PartialEq)]
pub struct RoomRelation {
    /// 房间 FRMW。面板不在册时（命名不合规房间的陈旧边）为 `None`。
    pub room: Option<RefU64>,
    /// 房号（`room_relate.room_num`，如 `R301`）。
    pub room_num: String,
    /// `{前缀}-{归一化房号}`（如 `1RX-R301`），与 `fn::room_code` 同构；
    /// 房间名解析不出前缀或房号长度算不出时为 `None`。
    pub room_code: Option<String>,
    /// 归属经由的面板（该房间下最强的那条边）。
    pub panel: RefU64,
    /// 元素 AABB 八顶点落在面板内的个数（0-8），主排序键。
    pub inside_count: u8,
    /// 元素中心到面板中心的距离，平局时越小越强。
    pub center_dist: f32,
    /// 这条归属来自直接子层成员而非元素自身（容器元素的聚合口径，决定 1）。
    pub via_member: bool,
}

/// 房间成员预览行（按归属强度排序后的前若干个）。
#[derive(Debug, Clone)]
pub struct RoomMemberPreview {
    pub refno: RefU64,
    pub name: String,
    pub noun: String,
    pub inside_count: u8,
}

/// 一间房的详情（「房间」页签的数据源）。
#[derive(Debug, Clone)]
pub struct RoomDetail {
    pub room: RefU64,
    pub name: String,
    pub room_num: String,
    pub room_code: Option<String>,
    pub panels: Vec<RefU64>,
    /// 去重后的成员总数（同一构件跨多面板只算一个）。
    pub member_count: usize,
    /// 去重后的**全量**成员 refno，按归属强度排序。房间视图的隔离 / 取景
    /// 目标集用它（预览截断后就凑不齐了）。
    pub member_refnos: Vec<RefU64>,
    /// 成员预览，按强度排序，长度受调用方 `preview` 上限约束。
    pub members: Vec<RoomMemberPreview>,
    /// 整间重算是否在队（只查 `room_recalc_panel` 的面板目标；元素目标反查
    /// 归属间接且清单可能上千，一期不做，见计划第六节）。
    pub pending_recalc: bool,
    /// 直段截断的候选分支（2026-09-29 直段截断计划 §4.5），按成员强度序去重。
    ///
    /// 由两部分组成：成员本身是 BRAN / HANG 的，加上成员的父是 BRAN / HANG 的那个父。直段没有自己的
    /// 参考号，以所属分支的 refno 渲染，所以截断只动这些分支名下的直段。第二部分不能省：旧的全量
    /// 重建路径不测直段，一根只有管件在册的分支得靠成员的父才认得出来。
    pub tube_carriers: Vec<RefU64>,
}

/// 一个成员往上一层的身份：自己的 noun、父的 refno 与 noun（截断候选的归并材料）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lineage {
    pub noun: Option<String>,
    pub owner: Option<RefU64>,
    pub owner_noun: Option<String>,
}

/// 直段挂在谁名下：E3D 里带隐含直管的只有管道分支与支吊架分支。
fn is_tube_carrier(noun: Option<&str>) -> bool {
    noun.map(str::trim)
        .is_some_and(|noun| noun.eq_ignore_ascii_case("BRAN") || noun.eq_ignore_ascii_case("HANG"))
}

/// 截断候选的归并（[`RoomDetail::tube_carriers`] 的口径）。按 `members` 的顺序走、首见去重：
/// 成员本身是 BRAN / HANG 的收自己，成员的父是 BRAN / HANG 的收那个父。`lineage` 里没有的成员
/// （库里查不到的陈旧边）跳过。
pub fn tube_carriers_of(members: &[RefU64], lineage: &HashMap<RefU64, Lineage>) -> Vec<RefU64> {
    let mut seen = HashSet::new();
    let mut carriers = Vec::new();
    for member in members {
        let Some(line) = lineage.get(member) else {
            continue;
        };
        if is_tube_carrier(line.noun.as_deref()) && seen.insert(*member) {
            carriers.push(*member);
        }
        if let Some(owner) = line.owner
            && is_tube_carrier(line.owner_noun.as_deref())
            && seen.insert(owner)
        {
            carriers.push(owner);
        }
    }
    carriers
}

/// 一次直选多少个成员。大房间的成员上千，一条语句里塞几千个记录 id 既难读也难排查，按块走。
const LINEAGE_CHUNK: usize = 1000;

/// 全部成员往上一层的身份（[`Lineage`]），按记录 id 直选，理由同 [`detail_follow_up_sql`]。
async fn member_lineage(members: &[RefU64]) -> Result<HashMap<RefU64, Lineage>> {
    let mut lineage = HashMap::with_capacity(members.len());
    for chunk in members.chunks(LINEAGE_CHUNK) {
        let mut response = SUL_DB.query(lineage_sql(chunk)).await?.check()?;
        let rows: Vec<(RefnoEnum, Option<String>, Option<RefnoEnum>, Option<String>)> =
            response.take(0)?;
        for (refno, noun, owner, owner_noun) in rows {
            lineage.insert(
                refno.refno(),
                Lineage {
                    noun,
                    owner: owner.map(|owner| owner.refno()),
                    owner_noun,
                },
            );
        }
    }
    Ok(lineage)
}

/// 房间浏览器的一行（S14）。
#[derive(Debug, Clone)]
pub struct RoomOverviewRow {
    pub room: RefU64,
    pub name: String,
    pub room_num: String,
    /// `{前缀}-{归一化房号}`，与 `fn::room_code` 同构；算不出时为 `None`
    /// （命名不合规的房间通常连码也解不出来）。
    pub room_code: Option<String>,
    /// 房号是否通过命名校验（不合规的不参与归属计算，仅在浏览器里暴露）。
    pub valid_name: bool,
    pub panel_count: usize,
    /// 去重后的成员数。
    pub member_count: usize,
    pub pending_recalc: bool,
}

/// 元素的全部房间归属，按归属强度排序（最强在前，即「主归属」）。
///
/// 自身边与直接子层聚合边一并查出；同一间房命中多条时保留最强一条，
/// `via_member` 记录最强那条是不是来自子层。
pub async fn element_rooms(refno: RefnoEnum) -> Result<Vec<RoomRelation>> {
    let pe_key = refno.to_pe_key();
    let mut response = SUL_DB.query(element_edges_sql(&pe_key)).await?.check()?;
    // 槛 0 / 1 是 LET $kids / LET $targets，边表在第 2 槛。
    let rows: Vec<(RefnoEnum, String, u8, f32, RefnoEnum)> = response.take(2)?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let merged = merge_edges(refno.refno(), rows);

    let panel_keys: Vec<String> = merged.iter().map(|e| e.panel.to_pe_key()).collect();
    let mut response = SUL_DB.query(panel_rooms_sql(&panel_keys)).await?.check()?;
    let panel_rooms: Vec<(RefnoEnum, RefnoEnum, Option<String>)> = response.take(0)?;
    let by_panel: HashMap<RefU64, (RefU64, Option<String>)> = panel_rooms
        .into_iter()
        .map(|(panel, room, name)| (panel.refno(), (room.refno(), name)))
        .collect();

    Ok(merged
        .into_iter()
        .map(|e| {
            let hit = by_panel.get(&e.panel);
            RoomRelation {
                room: hit.map(|(room, _)| *room),
                room_code: hit
                    .and_then(|(_, name)| name.as_deref())
                    .and_then(|name| room_code_of(name, &e.room_num)),
                room_num: e.room_num,
                panel: e.panel,
                inside_count: e.inside_count,
                center_dist: e.center_dist,
                via_member: e.via_member,
            }
        })
        .collect())
}

/// 一块在册 PANEL 直属的房间 FRMW。普通元素与陈旧面板返回 `None`。
/// `room_panel_relate` 的方向固定为 `in = 房间, out = PANEL`。
pub async fn panel_room(panel: RefnoEnum) -> Result<Option<RefU64>> {
    let mut response = SUL_DB
        .query(panel_room_sql(&panel.to_pe_key()))
        .await?
        .check()?;
    let rooms: Vec<RefnoEnum> = response.take(0)?;
    Ok(rooms.into_iter().next().map(|room| room.refno()))
}

/// 一间房直属的全部在册 PANEL。X-Ray 只需要这份轻量拓扑，不等待成员边全量展开。
pub async fn room_panels(room: RefnoEnum) -> Result<Vec<RefU64>> {
    let mut response = SUL_DB
        .query(room_panels_sql(&room.to_pe_key()))
        .await?
        .check()?;
    let panels: Vec<RefnoEnum> = response.take(0)?;
    Ok(panels.into_iter().map(|panel| panel.refno()).collect())
}

/// 一间房的详情。`room` 不是 pe 里的记录时返回 `Ok(None)`。
///
/// `preview` 是成员预览条数上限；`member_count` 始终是去重后的全量。
pub async fn room_detail(room: RefnoEnum, preview: usize) -> Result<Option<RoomDetail>> {
    let room_key = room.to_pe_key();
    let mut response = SUL_DB.query(room_detail_sql(&room_key)).await?.check()?;
    // 槛 0 是 LET $panels（与 element_edges_sql 同款），从 1 起才是结果。
    let names: Vec<Option<String>> = response.take(1)?;
    let Some(name) = names.into_iter().next().flatten() else {
        return Ok(None);
    };
    let panels: Vec<RefnoEnum> = response.take(2)?;
    let member_edges: Vec<(RefnoEnum, u8, f32)> = response.take(3)?;
    let panels: Vec<RefU64> = panels.into_iter().map(|p| p.refno()).collect();

    // 同一构件跨多面板只留最强的一条。
    let mut best: HashMap<RefU64, (u8, f32)> = HashMap::new();
    for (member, inside_count, center_dist) in member_edges {
        let member = member.refno();
        let candidate = (inside_count, center_dist);
        match best.get_mut(&member) {
            None => {
                best.insert(member, candidate);
            }
            Some(current) => {
                if strength_cmp(candidate, *current).is_lt() {
                    *current = candidate;
                }
            }
        }
    }
    let member_count = best.len();
    let mut ranked: Vec<(RefU64, u8, f32)> = best
        .into_iter()
        .map(|(refno, (inside, dist))| (refno, inside, dist))
        .collect();
    ranked.sort_by(|a, b| {
        strength_cmp((a.1, a.2), (b.1, b.2)).then_with(|| a.0.to_pdms_str().cmp(&b.0.to_pdms_str()))
    });
    let member_refnos: Vec<RefU64> = ranked.iter().map(|(refno, _, _)| *refno).collect();
    ranked.truncate(preview);

    let (member_names, pending_recalc) = if ranked.is_empty() && panels.is_empty() {
        (HashMap::new(), false)
    } else {
        let mut response = SUL_DB
            .query(detail_follow_up_sql(&ranked, &panels))
            .await?
            .check()?;
        let names: Vec<(RefnoEnum, Option<String>, Option<String>)> = response.take(0)?;
        let pending_hits: Vec<String> = response.take(1)?;
        let names: HashMap<RefU64, (Option<String>, Option<String>)> = names
            .into_iter()
            .map(|(refno, name, noun)| (refno.refno(), (name, noun)))
            .collect();
        (names, !pending_hits.is_empty())
    };

    let members = ranked
        .into_iter()
        .map(|(refno, inside_count, _)| {
            let (name, noun) = member_names.get(&refno).cloned().unwrap_or((None, None));
            let noun = noun.unwrap_or_default();
            let name = name.unwrap_or_else(|| format!("{} {}", noun, refno.to_pdms_str()));
            RoomMemberPreview {
                refno,
                name,
                noun,
                inside_count,
            }
        })
        .collect();

    let tube_carriers = if member_refnos.is_empty() {
        Vec::new()
    } else {
        tube_carriers_of(&member_refnos, &member_lineage(&member_refnos).await?)
    };

    let room_num = room_num_of_name(&name);
    Ok(Some(RoomDetail {
        room: room.refno(),
        room_code: room_code_of(&name, &room_num),
        name,
        room_num,
        panels,
        member_count,
        member_refnos,
        members,
        pending_recalc,
        tube_carriers,
    }))
}

/// 全部房间总览（S14 房间浏览器）。
///
/// 关键词取 `DbOption` 的 `room_key_word`（缺省 `["-RM"]`），凡名字命中的 FRMW
/// 都进结果——包括命名不合规、不参与归属计算的那些（`valid_name = false`）。
///
/// 这是全库扫描级查询（`room_relate` 全表边一次拉回），只该在打开浏览器时查，
/// 不要放进启动路径。
pub async fn rooms_overview() -> Result<Vec<RoomOverviewRow>> {
    let keywords = aios_core::get_db_option().get_room_key_word();
    let mut response = SUL_DB.query(overview_sql(&keywords)).await?.check()?;
    let rooms: Vec<(RefnoEnum, String)> = response.take(0)?;
    let room_panels: Vec<(RefnoEnum, RefnoEnum)> = response.take(1)?;
    let panel_members: Vec<(RefnoEnum, RefnoEnum)> = response.take(2)?;
    let pending_targets: Vec<String> = response.take(3)?;

    let mut panels_by_room: HashMap<RefU64, Vec<RefU64>> = HashMap::new();
    for (room, panel) in room_panels {
        panels_by_room
            .entry(room.refno())
            .or_default()
            .push(panel.refno());
    }
    let mut members_by_panel: HashMap<RefU64, Vec<RefU64>> = HashMap::new();
    for (panel, member) in panel_members {
        members_by_panel
            .entry(panel.refno())
            .or_default()
            .push(member.refno());
    }
    let pending: HashSet<String> = pending_targets.into_iter().collect();

    let mut result: Vec<RoomOverviewRow> = rooms
        .into_iter()
        .map(|(room, name)| {
            let room = room.refno();
            let panels = panels_by_room.get(&room).map(Vec::as_slice).unwrap_or(&[]);
            let members: HashSet<RefU64> = panels
                .iter()
                .filter_map(|panel| members_by_panel.get(panel))
                .flatten()
                .copied()
                .collect();
            let room_num = room_num_of_name(&name);
            RoomOverviewRow {
                room,
                valid_name: valid_room_num(&room_num),
                room_code: room_code_of(&name, &room_num),
                room_num,
                name,
                panel_count: panels.len(),
                member_count: members.len(),
                pending_recalc: panels
                    .iter()
                    .any(|panel| pending.contains(&panel.to_pdms_str())),
            }
        })
        .collect();
    result.sort_by(|a, b| {
        a.room_num
            .cmp(&b.room_num)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(result)
}

/// 归属强度全序（ADR-010 §5 前两键）：`inside_count` 降序 -> `center_dist` 升序。
/// 第三键（`room_num` / refno）由调用方按语境补。
fn strength_cmp(a: (u8, f32), b: (u8, f32)) -> std::cmp::Ordering {
    b.0.cmp(&a.0).then_with(|| a.1.total_cmp(&b.1))
}

#[derive(Debug, Clone, PartialEq)]
struct MergedEdge {
    panel: RefU64,
    room_num: String,
    inside_count: u8,
    center_dist: f32,
    via_member: bool,
}

/// 自身边 ∪ 子层聚合边 -> 每间房保留最强一条，再按强度排出全序。
fn merge_edges(
    self_refno: RefU64,
    rows: Vec<(RefnoEnum, String, u8, f32, RefnoEnum)>,
) -> Vec<MergedEdge> {
    let mut best: HashMap<String, MergedEdge> = HashMap::new();
    for (panel, room_num, inside_count, center_dist, holder) in rows {
        let candidate = MergedEdge {
            panel: panel.refno(),
            room_num,
            inside_count,
            center_dist,
            via_member: holder.refno() != self_refno,
        };
        match best.get_mut(&candidate.room_num) {
            None => {
                best.insert(candidate.room_num.clone(), candidate);
            }
            Some(current) => {
                // 平局保自身边：via_member 的强度信息一样时，「自己在房里」比
                // 「成员在房里」说得更直接。
                let ord = strength_cmp(
                    (candidate.inside_count, candidate.center_dist),
                    (current.inside_count, current.center_dist),
                )
                .then_with(|| candidate.via_member.cmp(&current.via_member));
                if ord.is_lt() {
                    *current = candidate;
                }
            }
        }
    }
    let mut merged: Vec<MergedEdge> = best.into_values().collect();
    merged.sort_by(|a, b| {
        strength_cmp(
            (a.inside_count, a.center_dist),
            (b.inside_count, b.center_dist),
        )
        .then_with(|| a.room_num.cmp(&b.room_num))
    });
    merged
}

/// 与 `fn::room_code` 同构：前缀 = 房间名去掉首 `/` 后按 `-` 分割的首段；
/// 房号 4 位原样，5 位去掉第 2 个字符（`RM301` -> `R301`），其余长度算不出。
fn room_code_of(room_name: &str, room_num: &str) -> Option<String> {
    let prefix = room_name
        .trim_start_matches('/')
        .split('-')
        .next()
        .filter(|s| !s.is_empty())?;
    if !room_num.is_ascii() {
        return None;
    }
    let code = match room_num.len() {
        4 => room_num.to_string(),
        5 => format!("{}{}", &room_num[..1], &room_num[2..]),
        _ => return None,
    };
    Some(format!("{prefix}-{code}"))
}

/// 房号 = 名字按 `-` 分割的末段（与 gen-model 建边时的口径一致）。
fn room_num_of_name(name: &str) -> String {
    name.rsplit('-').next().unwrap_or(name).to_string()
}

/// hd 命名规则 `^[A-Z]\d{3}$`（决定 4：一期写死，hh 项目接入时再配置化）。
fn valid_room_num(num: &str) -> bool {
    let bytes = num.as_bytes();
    bytes.len() == 4
        && bytes[0].is_ascii_uppercase()
        && bytes[1..].iter().all(|b| b.is_ascii_digit())
}

/// 自身边 ∪ 直接子层边，子层与边都按记录 id 图遍历（`X<-pe_owner`、`<-room_relate`），
/// 不写 `WHERE out = X`：SurrealDB 2.1 对 `pe_owner` / `room_relate` 的 `out` 不走任何索引，
/// AMS 两库 332k 条 `pe_owner` + 64k 条 `room_relate` 两次全表扫 1.7–2.6 s（子层为空、
/// 甚至记录不存在也一样），图遍历三句合计 0.02–0.03 s（2026-09-20 f13 隔离部署，34 个子 /
/// 79 条边等五组逐行比对一致）。选中元素后「归属加载中…」那 2.4–3.0 s 里 ~2.2 s 就是这两句。
///
/// 数组变量不能直接接图边——`$targets<-room_relate` 会少行——要 `SELECT VALUE <-room_relate
/// FROM $targets` 逐个取再 `array::flatten`。前两槛是 LET，边表在第 2 槛。
fn element_edges_sql(pe_key: &str) -> String {
    format!(
        "LET $kids = (SELECT VALUE in FROM {pe_key}<-pe_owner); \
         LET $targets = array::append($kids, {pe_key}); \
         SELECT VALUE [in, room_num, inside_count, center_dist, out] \
         FROM array::flatten(SELECT VALUE <-room_relate FROM $targets);"
    )
}

fn panel_rooms_sql(panel_keys: &[String]) -> String {
    format!(
        "SELECT VALUE [out, in, in.name] FROM room_panel_relate WHERE out IN [{}];",
        panel_keys.join(", ")
    )
}

fn panel_room_sql(panel_key: &str) -> String {
    format!("SELECT VALUE in FROM room_panel_relate WHERE out = {panel_key} LIMIT 1;")
}

fn room_panels_sql(room_key: &str) -> String {
    format!("SELECT VALUE out FROM room_panel_relate WHERE in = {room_key};")
}

/// 成员边按记录 id 图遍历收（`{panel}->room_relate`），**不写 `WHERE in IN $panels`**。
///
/// SurrealDB 2.1 在只有复合索引 `(in, out)` 的边表上执行 `in IN [数组]` 会静默返回 0 行：
/// `room_relate` 实测 0 行，而同一批边 `in = pe:…` 是 225 行、`->room_relate` 也是 225 行
/// （2026-10-25，ams 7997 现场；`room_panel_relate` / `pe_owner` 同样中招）。有单独 `in`
/// 索引的边表（`inst_relate` / `tubi_relate`）不受影响，别拿那两张当反例。
/// `IN (SELECT …)` 结果是对的，但对 `room_relate` 每行重算那个子查询：225 行实测 23 s
/// （2026-09-19 f20 曾记 146.6 s），同样不能用。图遍历同一份数据 2–3 ms。
fn room_detail_sql(room_key: &str) -> String {
    format!(
        "LET $panels = (SELECT VALUE out FROM room_panel_relate WHERE in = {room_key}); \
         SELECT VALUE name FROM {room_key}; \
         SELECT VALUE out FROM room_panel_relate WHERE in = {room_key}; \
         SELECT VALUE [out, inside_count, center_dist] \
         FROM array::flatten(SELECT VALUE ->room_relate FROM $panels);"
    )
}

/// 成员预览的名称补齐 + 整间重算在队检查，一趟发两条。
///
/// 名称那一句按记录 id 直选（`FROM pe:a, pe:b`），不写 `FROM pe WHERE id IN [...]`：
/// SurrealDB 2.1 对后者不走主键，把整张 `pe` 扫一遍——AMS 两库 240k 条实测 8 个 key 要
/// 1.6–1.8 s（空列表也要 1.0 s），直选 0.3 ms（2026-09-20 f13 隔离部署测出）。
/// 房间详情点开后的 2.2 s 里有 1.7 s 就是这一句。没有预览成员时用 `RETURN []` 占住
/// 第 0 槛，后面 `take(0)` 的位置不变。
fn detail_follow_up_sql(ranked: &[(RefU64, u8, f32)], panels: &[RefU64]) -> String {
    let member_keys = ranked
        .iter()
        .map(|(refno, _, _)| refno.to_pe_key())
        .collect::<Vec<_>>()
        .join(", ");
    let names_stmt = if member_keys.is_empty() {
        "RETURN [];".to_string()
    } else {
        format!("SELECT VALUE [id, name, noun] FROM {member_keys};")
    };
    let panel_targets = panels
        .iter()
        .map(|panel| format!("'{}'", panel.to_pdms_str()))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{names_stmt} \
         SELECT VALUE target_refno FROM model_update_pending WHERE action = 'room_recalc_panel' \
         AND status IN ['pending', 'failed'] AND (attempts?:0) < 5 \
         AND target_refno IN [{panel_targets}];"
    )
}

/// 一块成员往上一层的身份：自己的 noun、父、父的 noun。按记录 id 直选（`FROM pe:a, pe:b`），
/// 不写 `FROM pe WHERE id IN [...]`，理由见 [`detail_follow_up_sql`]。调用方保证 `members` 非空。
fn lineage_sql(members: &[RefU64]) -> String {
    let keys = members
        .iter()
        .map(|refno| refno.to_pe_key())
        .collect::<Vec<_>>()
        .join(", ");
    format!("SELECT VALUE [id, noun, owner, owner.noun] FROM {keys};")
}

fn overview_sql(keywords: &[String]) -> String {
    let filter = keywords
        .iter()
        .map(|kw| format!("string::contains(name, '{}')", kw.replace('\'', "\\'")))
        .collect::<Vec<_>>()
        .join(" OR ");
    format!(
        "SELECT VALUE [id, name] FROM pe WHERE noun = 'FRMW' AND ({filter}); \
         SELECT VALUE [in, out] FROM room_panel_relate; \
         SELECT VALUE [in, out] FROM room_relate; \
         SELECT VALUE target_refno FROM model_update_pending WHERE action = 'room_recalc_panel' \
         AND status IN ['pending', 'failed'] AND (attempts?:0) < 5;"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refno(n: u64) -> RefU64 {
        RefU64(n)
    }

    fn edge(
        panel: u64,
        room_num: &str,
        inside: u8,
        dist: f32,
        holder: u64,
    ) -> (RefnoEnum, String, u8, f32, RefnoEnum) {
        (
            RefnoEnum::Refno(refno(panel)),
            room_num.to_string(),
            inside,
            dist,
            RefnoEnum::Refno(refno(holder)),
        )
    }

    #[test]
    fn room_code_follows_fn_room_code_shape() {
        assert_eq!(
            room_code_of("/1RX-RM03-R301", "R301"),
            Some("1RX-R301".to_string())
        );
        // 5 位房号去掉第 2 个字符。
        assert_eq!(
            room_code_of("/1RX-RM03-RM301", "RM301"),
            Some("1RX-R301".to_string())
        );
        // 其余长度算不出。
        assert_eq!(room_code_of("/1RX-RM03-R30", "R30"), None);
        // 前缀解析不出。
        assert_eq!(room_code_of("/", "R301"), None);
        // 非 ASCII 不切片（防 panic）。
        assert_eq!(room_code_of("/1RX-房间", "房间号五个"), None);
    }

    #[test]
    fn room_num_validation_is_hd_rule() {
        assert!(valid_room_num("R301"));
        assert!(valid_room_num("A000"));
        assert!(!valid_room_num("RM12"));
        assert!(!valid_room_num("r301"));
        assert!(!valid_room_num("R30"));
        assert!(!valid_room_num("R3011"));
        assert!(!valid_room_num(""));
    }

    #[test]
    fn room_num_is_last_dash_segment() {
        assert_eq!(room_num_of_name("/1RX-RM03-R301"), "R301");
        assert_eq!(room_num_of_name("NO-DASH-TAIL-"), "");
        assert_eq!(room_num_of_name("PLAIN"), "PLAIN");
    }

    #[test]
    fn merge_keeps_strongest_edge_per_room_and_orders_by_strength() {
        let this = refno(1);
        // R301 有两条边（弱自身边 + 强自身边），R302 一条弱边，R300 与 R301 的
        // 最强边同强度——按 room_num 升序排在 R301 前面。
        let rows = vec![
            edge(11, "R301", 4, 9.0, 1),
            edge(12, "R301", 8, 1.2, 1),
            edge(13, "R302", 2, 5.6, 1),
            edge(14, "R300", 8, 1.2, 1),
        ];
        let merged = merge_edges(this, rows);
        assert_eq!(
            merged
                .iter()
                .map(|e| (e.room_num.as_str(), e.panel.0, e.inside_count))
                .collect::<Vec<_>>(),
            vec![("R300", 14, 8), ("R301", 12, 8), ("R302", 13, 2)],
        );
    }

    #[test]
    fn merge_breaks_inside_tie_by_smaller_distance() {
        let merged = merge_edges(
            refno(1),
            vec![edge(11, "R301", 6, 3.0, 1), edge(12, "R301", 6, 1.5, 1)],
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].panel, refno(12));
    }

    #[test]
    fn merge_marks_member_sourced_edges_and_prefers_self_on_tie() {
        let this = refno(1);
        // R301 只有子层边 -> via_member；R302 自身边与子层边完全同强 -> 保自身。
        let rows = vec![
            edge(11, "R301", 5, 2.0, 42),
            edge(12, "R302", 3, 4.0, 43),
            edge(13, "R302", 3, 4.0, 1),
        ];
        let merged = merge_edges(this, rows);
        let r301 = merged.iter().find(|e| e.room_num == "R301").unwrap();
        let r302 = merged.iter().find(|e| e.room_num == "R302").unwrap();
        assert!(r301.via_member);
        assert!(!r302.via_member);
        assert_eq!(r302.panel, refno(13));
    }

    #[test]
    fn strength_ordering_matches_adr010() {
        // inside 降序优先。
        assert!(strength_cmp((8, 9.0), (7, 0.1)).is_lt());
        // 平局距离升序。
        assert!(strength_cmp((6, 1.0), (6, 2.0)).is_lt());
        assert_eq!(strength_cmp((6, 1.0), (6, 1.0)), std::cmp::Ordering::Equal);
    }

    #[test]
    fn sql_renderers_pin_tables_and_predicates() {
        let edges = element_edges_sql("pe:1_2");
        // 子层与归属边都按记录 id 图遍历，不许再写 WHERE out = …（SurrealDB 2.1 全表扫
        // pe_owner / room_relate）；前两槛是 LET，element_rooms() 从第 2 槛取边表。
        assert!(edges.starts_with("LET $kids = (SELECT VALUE in FROM pe:1_2<-pe_owner);"));
        assert!(edges.contains("LET $targets = array::append($kids, pe:1_2);"));
        assert!(edges.contains(
            "SELECT VALUE [in, room_num, inside_count, center_dist, out] \
             FROM array::flatten(SELECT VALUE <-room_relate FROM $targets);"
        ));
        assert!(!edges.contains("WHERE out ="));
        assert!(!edges.contains("FROM room_relate WHERE"));
        assert_eq!(edges.matches("LET ").count(), 2);

        let panels = panel_rooms_sql(&["pe:1_10".to_string(), "pe:1_11".to_string()]);
        assert!(panels.contains("FROM room_panel_relate WHERE out IN [pe:1_10, pe:1_11]"));
        assert!(panels.contains("in.name"));

        let detail = room_detail_sql("pe:1_5");
        assert!(detail.contains("SELECT VALUE name FROM pe:1_5"));
        assert!(detail.contains("FROM room_panel_relate WHERE in = pe:1_5"));
        assert!(detail.starts_with("LET $panels = (SELECT VALUE out FROM room_panel_relate WHERE in = pe:1_5);"));
        // 成员边按边目标图遍历收。`WHERE in IN $panels` 在这张边表上恒 0 行（见渲染函数的注释），
        // 而它曾经是「成员 0 / 截断 0 段」的成因，钉住这个形状就是钉住那个回归。
        assert!(
            detail.contains(
                "SELECT VALUE [out, inside_count, center_dist] \
                 FROM array::flatten(SELECT VALUE ->room_relate FROM $panels);"
            ),
            "{detail}"
        );
        assert!(!detail.contains("WHERE in IN"), "{detail}");
        assert!(!detail.contains("IN (SELECT"), "{detail}");

        let follow = detail_follow_up_sql(&[(refno(7), 8, 1.0), (refno(8), 7, 2.0)], &[refno(9)]);
        // 名称按记录 id 直选，不许再写 WHERE id IN（SurrealDB 2.1 会全表扫 pe）。
        assert!(follow.starts_with("SELECT VALUE [id, name, noun] FROM pe:0_7, pe:0_8;"));
        assert!(!follow.contains("WHERE id IN"));
        assert!(follow.contains("action = 'room_recalc_panel'"));
        assert!(follow.contains("(attempts?:0) < 5"));
        assert!(follow.contains("target_refno IN ['0/9']"));

        // 没有预览成员时第 0 槛用 RETURN [] 占位，take(0) 的位置不动，也不会拼出 `FROM ;`。
        let follow_empty = detail_follow_up_sql(&[], &[refno(9)]);
        assert!(follow_empty.starts_with("RETURN [];"));
        assert!(follow_empty.contains("target_refno IN ['0/9']"));

        let overview = overview_sql(&["-RM".to_string()]);
        assert!(overview.contains("noun = 'FRMW'"));
        assert!(overview.contains("string::contains(name, '-RM')"));
        assert!(overview.contains("SELECT VALUE [in, out] FROM room_relate"));
    }

    #[test]
    fn panel_room_query_uses_the_canonical_topology_direction() {
        let sql = panel_room_sql("pe:1_10");
        assert!(sql.contains("SELECT VALUE in FROM room_panel_relate"));
        assert!(sql.contains("WHERE out = pe:1_10"));
        assert!(sql.contains("LIMIT 1"));
    }

    fn lineage(noun: &str, owner: Option<(u64, &str)>) -> Lineage {
        Lineage {
            noun: Some(noun.to_string()),
            owner: owner.map(|(refno_of_owner, _)| refno(refno_of_owner)),
            owner_noun: owner.map(|(_, owner_noun)| owner_noun.to_string()),
        }
    }

    /// 截断候选 = 成员里的 BRAN / HANG ∪ 成员的 BRAN / HANG 父，按成员强度序首见去重。
    /// 设备零件的父是 EQUI，不进来；库里查不到的成员跳过。
    #[test]
    fn tube_carriers_are_pipe_members_and_pipe_parents_in_rank_order() {
        let (bran_member, bran, elbo, atta) = (refno(40), refno(11), refno(10), refno(12));
        let (hang, clamp, equi_part, stale) = (refno(20), refno(21), refno(30), refno(99));
        let members = vec![bran_member, elbo, atta, clamp, equi_part, stale];
        let lineage = HashMap::from([
            (bran_member, lineage("BRAN", Some((41, "PIPE")))),
            (elbo, lineage("ELBO", Some((11, "BRAN")))),
            (atta, lineage("ATTA", Some((11, "BRAN")))),
            (clamp, lineage("PCLA", Some((20, "hang")))),
            (equi_part, lineage("CYLI", Some((31, "EQUI")))),
        ]);

        assert_eq!(
            tube_carriers_of(&members, &lineage),
            vec![bran_member, bran, hang]
        );
    }

    /// 成员本身是分支、它的父又是分支（HANG 挂在 BRAN 下之类）：两个都收，各一次。
    #[test]
    fn a_carrier_member_and_its_carrier_parent_both_count_once() {
        let (hang, bran) = (refno(1), refno(2));
        let lineage = HashMap::from([
            (hang, lineage("HANG", Some((2, "BRAN")))),
            (bran, lineage("BRAN", Some((3, "PIPE")))),
        ]);
        assert_eq!(tube_carriers_of(&[hang, bran], &lineage), vec![hang, bran]);
        assert!(tube_carriers_of(&[], &lineage).is_empty());
    }

    /// 直选按记录 id，不回退成整表扫；四格顺序就是反序列化的元组顺序。
    #[test]
    fn lineage_sql_selects_members_by_record_id() {
        let sql = lineage_sql(&[refno(0x1_0000_0002), refno(0x3_0000_0004)]);
        assert!(sql.starts_with("SELECT VALUE [id, noun, owner, owner.noun] FROM "), "{sql}");
        assert!(!sql.contains("WHERE"), "{sql}");
        assert_eq!(sql.matches("pe:").count(), 2, "{sql}");
    }

    #[test]
    fn room_panels_query_uses_the_canonical_topology_direction() {
        let sql = room_panels_sql("pe:1_9");
        assert_eq!(
            sql,
            "SELECT VALUE out FROM room_panel_relate WHERE in = pe:1_9;"
        );
    }
}
