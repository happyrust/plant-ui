//! 虚拟孔洞会话状态机（ADR-0031 阶段一 / 阶段四的编排账）。
//!
//! 纯状态、不碰 IO：导入结果、映射回包、布尔回执都从外面喂进来，这里只管
//! 状态转移、帧号失效与向 Vm 的投影。文件读取在 `virtual_hole`（导入器），
//! 库里确认目标在数据线程（`Req::VirtualHoleTargets`），布尔作业在视口
//! （`plant-ui-view3d` 的预览层）——三家合起来才是一趟预览。
//!
//! 孔洞用 `id`（记录的 `Code`）做键。AMS 表的 `Code` 允许空、允许重复，
//! 导入时会就地摊平补齐（空的补 `行N`，重复的加 `#2` 后缀），保证一份会话
//! 里的 id 唯一——命令都按 id 走，重复就说不清点的是哪一条。

use std::collections::{HashMap, HashSet};

use aios_core::RefU64;
use plant_ui::virtual_hole::{VirtualHole, VirtualHoleReceipt, VirtualHoleStatus};
use plant_ui::vm::{VirtualHoleRowVm, VirtualHoleTargetVm, VirtualHolesVm};

use crate::virtual_hole::ImportReport;

/// 一条孔洞的映射状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoleMapping {
    /// `RelyItemREF` 没解析出参考号——只能手动映射。
    Unparsed,
    /// 参考号已解析，正在库里确认（`Req::VirtualHoleTargets` 在途）。
    Resolving(RefU64),
    /// 目标已确认在库里（自动确认或手动指定）。
    Mapped(RefU64),
    /// 参考号解析出了，但库里没有这个元素——只能手动映射。
    Missing(RefU64),
}

/// 一份布尔回执落地之后，宿主还要补做的事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptEffect {
    /// 行状态已更新，宿主什么都不用补做。
    StatusOnly,
    /// 目标在场景里一个实例都没有：宿主去显示它，落地后拿同一批孔洞重试
    /// （每目标只重试一次，见 `App::hole_target_retried`）。
    RetryAfterShow(RefU64),
}

/// 一趟虚拟孔洞预览的会话。孔洞列表与映射/布尔状态都在这里，内存만、
/// 不写回正式模型；清掉预览只恢复三维画面，列表本身要重导入才换。
#[derive(Debug, Default)]
pub struct VirtualHoleSession {
    holes: Vec<VirtualHole>,
    /// 行级解析错误（行号从 1 开始，含表头行）。
    row_errors: Vec<(usize, String)>,
    /// hole id -> 映射状态。
    mappings: HashMap<String, HoleMapping>,
    /// hole id -> 布尔状态（不在表里的行恒是 Ready）。
    statuses: HashMap<String, VirtualHoleStatus>,
    /// 预览帧号：每次清预览 / 重导入 / 整清都进一帧，旧回执拿它丢弃。
    epoch: u64,
    /// 映射确认帧号：只在重导入时进一帧。清预览**不**进这帧——映射在清掉
    /// 预览之后仍然有效（ADR-0031 阶段一）。宿主发 `Req::VirtualHoleTargets`
    /// 时带的是它自己的 `hole_map_epoch`，两帧只随重导入同拍各 +1，始终相等。
    map_epoch: u64,
    selected: Option<String>,
    /// 「看原始网格」开关（只对已出结果的实体有意义）。
    show_original: bool,
    /// 孔洞预览 X-Ray 开关。
    xray: bool,
}

impl VirtualHoleSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.holes.is_empty() && self.row_errors.is_empty()
    }

    /// 会话里的全部孔洞（导入顺序）。宿主拿去拼 `SetLayer` 与预览作业。
    pub fn holes(&self) -> &[VirtualHole] {
        &self.holes
    }

    /// 按 id 找一条孔洞。
    pub fn hole(&self, hole_id: &str) -> Option<&VirtualHole> {
        self.holes.iter().find(|hole| hole.id == hole_id)
    }

    /// 一条孔洞当前映射到的目标（已确认存在的才算）。
    pub fn target_of(&self, hole_id: &str) -> Option<RefU64> {
        match self.mappings.get(hole_id) {
            Some(HoleMapping::Mapped(refno)) => Some(*refno),
            _ => None,
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// 导入一批解析结果：整份替换。旧会话的在途回执靠帧号作废
    /// （这里进一帧），宿主同时要发 `ClearVirtualHolePreview` 撤三维那一层。
    pub fn import(&mut self, report: ImportReport) {
        let ImportReport {
            mut holes,
            row_errors,
        } = report;
        unique_ids(&mut holes);
        self.mappings = holes
            .iter()
            .map(|hole| {
                let mapping = match hole.rely_refno {
                    Some(refno) => HoleMapping::Resolving(refno),
                    None => HoleMapping::Unparsed,
                };
                (hole.id.clone(), mapping)
            })
            .collect();
        self.statuses.clear();
        self.holes = holes;
        self.row_errors = row_errors;
        self.selected = None;
        self.show_original = false;
        self.epoch = self.epoch.wrapping_add(1);
        self.map_epoch = self.map_epoch.wrapping_add(1);
    }

    /// 自动映射要去库里确认的参考号名单（去重、保持孔洞顺序）。
    /// 手动映射过的行已经是 `Mapped`，不再重复查。
    pub fn pending_mapping_refnos(&self) -> Vec<RefU64> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for hole in &self.holes {
            if let Some(HoleMapping::Resolving(refno)) = self.mappings.get(&hole.id)
                && seen.insert(*refno)
            {
                out.push(*refno);
            }
        }
        out
    }

    /// 映射回包：`map_epoch` 对不上当前帧的整包丢弃（上一份导入之后回来的）。
    /// 清预览不进映射帧，清掉预览之后回来的映射回包照常落地——映射与预览
    /// 是两笔账，拿预览帧（`epoch`）认映射回包会在清过一次之后把重导入的
    /// 自动映射整包丢掉，行卡在「确认目标中」。
    /// 返回 true 表示这包被接收了。
    pub fn apply_targets(&mut self, epoch: u64, results: &[(RefU64, bool)]) -> bool {
        if epoch != self.map_epoch {
            return false;
        }
        for (refno, exists) in results {
            for hole in &self.holes {
                if self.mappings.get(&hole.id) != Some(&HoleMapping::Resolving(*refno)) {
                    continue;
                }
                let id = hole.id.clone();
                let mapping = if *exists {
                    HoleMapping::Mapped(*refno)
                } else {
                    HoleMapping::Missing(*refno)
                };
                self.mappings.insert(id, mapping);
            }
        }
        true
    }

    /// 手动映射：目标来自模型树选中，天然在库里，直接落 `Mapped`。
    /// 孔洞不存在时返回 false。
    pub fn map_manually(&mut self, hole_id: &str, refno: RefU64) -> bool {
        if !self.holes.iter().any(|hole| hole.id == hole_id) {
            return false;
        }
        self.mappings
            .insert(hole_id.to_string(), HoleMapping::Mapped(refno));
        // 手动改映射之后，旧的布尔状态不再有意义——回到 Ready 等下一趟预览。
        self.statuses.remove(hole_id);
        true
    }

    /// 选中一条记录（单选）；`None` 清掉选中。
    pub fn select(&mut self, hole_id: Option<&str>) {
        self.selected = hole_id
            .filter(|id| self.holes.iter().any(|hole| hole.id == *id))
            .map(str::to_string);
    }

    /// 发起整趟预览：把「已映射且还没进布尔」的行翻成 Calculating，
    /// 返回需要宿主拿去做布尔的目标名单（去重、保持孔洞顺序）。
    /// 没有可算的行时返回空——宿主据此什么都不发。
    pub fn begin_preview(&mut self) -> Vec<RefU64> {
        let mut targets = Vec::new();
        let mut seen = HashSet::new();
        for hole in &self.holes {
            let Some(HoleMapping::Mapped(refno)) = self.mappings.get(&hole.id) else {
                continue;
            };
            let status = self
                .statuses
                .get(&hole.id)
                .copied()
                .unwrap_or(VirtualHoleStatus::Ready);
            if status != VirtualHoleStatus::Ready {
                continue;
            }
            self.statuses
                .insert(hole.id.clone(), VirtualHoleStatus::Calculating);
            if seen.insert(*refno) {
                targets.push(*refno);
            }
        }
        targets
    }

    /// 一个目标下的全部孔洞（宿主拿去拼布尔作业）。
    pub fn holes_for_target(&self, refno: RefU64) -> Vec<&VirtualHole> {
        self.holes
            .iter()
            .filter(|hole| self.mappings.get(&hole.id) == Some(&HoleMapping::Mapped(refno)))
            .collect()
    }

    /// 布尔回执落地。帧号对不上整包丢弃；`no_instances` 的回执把行回拨到
    /// Ready（还没真正算过），并请宿主先显示目标再重试一次。
    pub fn apply_receipt(&mut self, receipt: &VirtualHoleReceipt) -> ReceiptEffect {
        if receipt.epoch != self.epoch {
            return ReceiptEffect::StatusOnly;
        }
        for hole_id in &receipt.hole_ids {
            if receipt.no_instances {
                self.statuses.remove(hole_id);
            } else if receipt.dropped {
                self.statuses.insert(
                    hole_id.clone(),
                    VirtualHoleStatus::Failed,
                );
            } else {
                self.statuses
                    .insert(hole_id.clone(), receipt.status);
            }
        }
        if receipt.no_instances {
            ReceiptEffect::RetryAfterShow(receipt.refno)
        } else {
            ReceiptEffect::StatusOnly
        }
    }

    /// 清掉预览层：行状态回拨到 Ready（映射保留），进一帧作废在途回执。
    /// 宿主同时拿这帧号去撤视口的切削体与恢复原始网格。
    pub fn clear_preview(&mut self) {
        self.statuses.clear();
        self.show_original = false;
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// 整清：重连 / 换读面时孔洞列表一起作废（旧库的参考号在新库里没意义）。
    pub fn reset(&mut self) {
        self.holes.clear();
        self.row_errors.clear();
        self.mappings.clear();
        self.statuses.clear();
        self.selected = None;
        self.show_original = false;
        self.xray = false;
        self.epoch = self.epoch.wrapping_add(1);
    }

    pub fn set_show_original(&mut self, on: bool) {
        self.show_original = on;
    }

    pub fn set_xray(&mut self, on: bool) {
        self.xray = on;
    }

    pub fn xray(&self) -> bool {
        self.xray
    }

    /// 投影到「虚拟孔洞」页签的只读 Vm。
    pub fn vm(&self) -> VirtualHolesVm {
        let holes = self
            .holes
            .iter()
            .map(|hole| {
                let target = match self.mappings.get(&hole.id) {
                    Some(HoleMapping::Mapped(refno)) => VirtualHoleTargetVm::Mapped(*refno),
                    Some(HoleMapping::Resolving(_)) => VirtualHoleTargetVm::Resolving,
                    Some(HoleMapping::Missing(_)) => VirtualHoleTargetVm::Missing,
                    Some(HoleMapping::Unparsed) | None => VirtualHoleTargetVm::Unparsed,
                };
                VirtualHoleRowVm {
                    id: hole.id.clone(),
                    size_label: hole.size_label(),
                    target,
                    status: self
                        .statuses
                        .get(&hole.id)
                        .copied()
                        .unwrap_or(VirtualHoleStatus::Ready),
                }
            })
            .collect();
        let mut calculating = 0;
        let mut done = 0;
        for status in self.statuses.values() {
            match status {
                VirtualHoleStatus::Calculating => calculating += 1,
                VirtualHoleStatus::Changed
                | VirtualHoleStatus::NoIntersection
                | VirtualHoleStatus::Failed => done += 1,
                VirtualHoleStatus::Ready => {}
            }
        }
        VirtualHolesVm {
            holes,
            row_errors: self.row_errors.clone(),
            selected: self.selected.clone(),
            show_original: self.show_original,
            xray: self.xray,
            calculating,
            done,
        }
    }
}

/// 摊平补齐孔洞 id：空的补 `行N`（N = 来源行号），重复的加 `#2`、`#3` 后缀。
/// 一份会话里 id 必须唯一——命令按 id 走，重复就说不清点的是哪一条。
fn unique_ids(holes: &mut [VirtualHole]) {
    let mut seen: HashSet<String> = HashSet::new();
    for hole in holes.iter_mut() {
        let base = if hole.id.is_empty() {
            format!("行{}", hole.source_row)
        } else {
            hole.id.clone()
        };
        let mut candidate = base.clone();
        let mut n = 2;
        while !seen.insert(candidate.clone()) {
            candidate = format!("{base}#{n}");
            n += 1;
        }
        hole.id = candidate;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plant_ui::virtual_hole::{VirtualHoleShape, parse_rely_refno};

    fn hole(id: &str, row: usize, rely: &str) -> VirtualHole {
        VirtualHole {
            id: id.to_string(),
            shape: VirtualHoleShape::Circle,
            center_mm: [0.0, 0.0, 0.0],
            orientation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            diameter_mm: Some(300.0),
            width_mm: None,
            height_mm: None,
            depth_mm: 800.0,
            rely_item_ref: rely.to_string(),
            rely_refno: parse_rely_refno(rely),
            item_ref: String::new(),
            source_row: row,
            hole_work: String::new(),
            h_type: String::new(),
            subs_material: String::new(),
            raw_shape: "CIR".into(),
            raw_orientation: String::new(),
        }
    }

    fn report(holes: Vec<VirtualHole>) -> ImportReport {
        ImportReport {
            holes,
            row_errors: Vec::new(),
        }
    }

    fn refno(raw: &str) -> RefU64 {
        parse_rely_refno(raw).unwrap()
    }

    #[test]
    fn import_normalizes_empty_and_duplicate_ids() {
        let mut session = VirtualHoleSession::new();
        session.import(report(vec![
            hole("", 2, "24381_145018"),
            hole("H1", 3, "24381_145019"),
            hole("H1", 4, "24381_145020"),
        ]));
        let ids: Vec<&str> = session.holes.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["行2", "H1", "H1#2"]);
    }

    #[test]
    fn mapping_transitions_resolving_to_mapped_or_missing() {
        let mut session = VirtualHoleSession::new();
        session.import(report(vec![
            hole("A", 2, "24381_145018"),
            hole("B", 3, "24381_145019"),
            hole("C", 4, "not-a-refno"),
        ]));
        // C 没解析出参考号，不在查询名单里。
        assert_eq!(
            session.pending_mapping_refnos(),
            vec![refno("24381_145018"), refno("24381_145019")]
        );
        assert!(session.apply_targets(
            session.epoch(),
            &[(refno("24381_145018"), true), (refno("24381_145019"), false)]
        ));
        let vm = session.vm();
        assert_eq!(vm.holes[0].target, VirtualHoleTargetVm::Mapped(refno("24381_145018")));
        assert_eq!(vm.holes[1].target, VirtualHoleTargetVm::Missing);
        assert_eq!(vm.holes[2].target, VirtualHoleTargetVm::Unparsed);
    }

    #[test]
    fn stale_epoch_targets_are_dropped() {
        let mut session = VirtualHoleSession::new();
        session.import(report(vec![hole("A", 2, "24381_145018")]));
        let stale = session.epoch().wrapping_add(1);
        assert!(!session.apply_targets(stale, &[(refno("24381_145018"), true)]));
        // 还是 Resolving：过期回包没生效。
        assert_eq!(session.vm().holes[0].target, VirtualHoleTargetVm::Resolving);
    }

    #[test]
    fn mapping_receipt_survives_clear_preview() {
        // 回归（2026-10-15 AMS 验收现场）：宿主拿自己的映射帧号发确认请求，
        // 只随重导入走；会话这边要是拿预览帧（清预览也进帧）去认，清过一次
        // 预览之后重导入的自动映射就会整包丢掉，目标芯片卡在「确认目标中」。
        let mut session = VirtualHoleSession::new();
        session.import(report(vec![hole("A", 2, "24381_145018")]));
        // 清预览：预览帧进一帧，映射帧不动。
        session.clear_preview();
        // 清掉预览之后回来的映射回包要能落地。
        assert!(session.apply_targets(1, &[(refno("24381_145018"), true)]));
        assert_eq!(session.target_of("A"), Some(refno("24381_145018")));
        // 重导入进新帧：旧帧回包整包丢弃，新帧的才能落地。
        session.import(report(vec![hole("A", 2, "24381_145018")]));
        assert!(!session.apply_targets(1, &[(refno("24381_145018"), true)]));
        assert!(session.apply_targets(2, &[(refno("24381_145018"), true)]));
        assert_eq!(session.target_of("A"), Some(refno("24381_145018")));
    }

    #[test]
    fn begin_preview_only_takes_ready_mapped_holes() {
        let mut session = VirtualHoleSession::new();
        session.import(report(vec![
            hole("A", 2, "24381_145018"),
            hole("B", 3, "24381_145018"),
            hole("C", 4, "24381_145019"),
        ]));
        session.apply_targets(
            session.epoch(),
            &[(refno("24381_145018"), true), (refno("24381_145019"), true)],
        );
        // 目标去重：两个孔洞同属一个实体只报一次。
        assert_eq!(session.begin_preview(), vec![refno("24381_145018"), refno("24381_145019")]);
        assert_eq!(session.holes_for_target(refno("24381_145018")).len(), 2);
        let vm = session.vm();
        assert_eq!(vm.calculating, 3);
        assert_eq!(vm.done, 0);
        // 第二次按：没有 Ready 的行，空名单。
        assert!(session.begin_preview().is_empty());
    }

    #[test]
    fn receipt_updates_statuses_and_retry_rolls_back() {
        let mut session = VirtualHoleSession::new();
        session.import(report(vec![
            hole("A", 2, "24381_145018"),
            hole("B", 3, "24381_145019"),
        ]));
        session.apply_targets(
            session.epoch(),
            &[(refno("24381_145018"), true), (refno("24381_145019"), true)],
        );
        session.begin_preview();
        let epoch = session.epoch();
        // 目标没实例：回拨 Ready，请宿主显示后重试。
        let receipt = VirtualHoleReceipt {
            epoch,
            refno: refno("24381_145018"),
            hole_ids: vec!["A".into()],
            status: VirtualHoleStatus::Calculating,
            no_instances: true,
            dropped: false,
            error: None,
            elapsed_ms: 0,
        };
        assert_eq!(
            session.apply_receipt(&receipt),
            ReceiptEffect::RetryAfterShow(refno("24381_145018"))
        );
        assert_eq!(session.vm().holes[0].status, VirtualHoleStatus::Ready);
        // 正常回执：状态落定。
        let receipt = VirtualHoleReceipt {
            epoch,
            refno: refno("24381_145019"),
            hole_ids: vec!["B".into()],
            status: VirtualHoleStatus::Changed,
            no_instances: false,
            dropped: false,
            error: None,
            elapsed_ms: 12,
        };
        assert_eq!(session.apply_receipt(&receipt), ReceiptEffect::StatusOnly);
        let vm = session.vm();
        assert_eq!(vm.holes[1].status, VirtualHoleStatus::Changed);
        assert_eq!(vm.done, 1);
        assert_eq!(vm.calculating, 0);
    }

    #[test]
    fn stale_epoch_receipt_is_ignored() {
        let mut session = VirtualHoleSession::new();
        session.import(report(vec![hole("A", 2, "24381_145018")]));
        session.apply_targets(session.epoch(), &[(refno("24381_145018"), true)]);
        session.begin_preview();
        let stale = session.epoch().wrapping_add(1);
        let receipt = VirtualHoleReceipt {
            epoch: stale,
            refno: refno("24381_145018"),
            hole_ids: vec!["A".into()],
            status: VirtualHoleStatus::Changed,
            no_instances: false,
            dropped: false,
            error: None,
            elapsed_ms: 0,
        };
        assert_eq!(session.apply_receipt(&receipt), ReceiptEffect::StatusOnly);
        // 还是 Calculating：过期回执没落状态。
        assert_eq!(session.vm().holes[0].status, VirtualHoleStatus::Calculating);
    }

    #[test]
    fn clear_preview_rolls_statuses_back_and_bumps_epoch() {
        let mut session = VirtualHoleSession::new();
        session.import(report(vec![hole("A", 2, "24381_145018")]));
        session.apply_targets(session.epoch(), &[(refno("24381_145018"), true)]);
        session.begin_preview();
        let before = session.epoch();
        session.clear_preview();
        assert!(session.epoch() > before);
        let vm = session.vm();
        assert_eq!(vm.holes[0].status, VirtualHoleStatus::Ready);
        // 映射保留：再按预览还能发同一个目标。
        assert_eq!(session.begin_preview(), vec![refno("24381_145018")]);
    }

    #[test]
    fn reset_drops_everything() {
        let mut session = VirtualHoleSession::new();
        session.import(report(vec![hole("A", 2, "24381_145018")]));
        session.apply_targets(session.epoch(), &[(refno("24381_145018"), true)]);
        session.begin_preview();
        session.reset();
        assert!(session.is_empty());
        assert!(session.vm().holes.is_empty());
        assert!(session.begin_preview().is_empty());
    }

    #[test]
    fn manual_mapping_beats_unparsed_and_missing() {
        let mut session = VirtualHoleSession::new();
        session.import(report(vec![
            hole("A", 2, "junk"),
            hole("B", 3, "24381_145019"),
        ]));
        session.apply_targets(session.epoch(), &[(refno("24381_145019"), false)]);
        let manual = refno("24381_999999");
        assert!(session.map_manually("A", manual));
        assert!(!session.map_manually("nope", manual));
        let vm = session.vm();
        assert_eq!(vm.holes[0].target, VirtualHoleTargetVm::Mapped(manual));
        assert_eq!(vm.holes[1].target, VirtualHoleTargetVm::Missing);
        // 手动映射的行立刻就能进预览名单。
        assert!(session.begin_preview().contains(&manual));
    }
}
