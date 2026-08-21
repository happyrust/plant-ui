//! 「重新生成模型」的纯逻辑：把已生成元素归并成生成根、把服务端回执分档。
//!
//! 这里一行 IO 都不做，全部可单测。状态机与接线在 `main.rs`。

use std::collections::{HashMap, HashSet};

use plant_ui::model_update::Failure;
use plant_ui_data::{GeneratedScope, RefU64};

use crate::model_update_api::EnsureStatus;

/// 恒不做生成根的粗层级名词。**只有一处用到**：判断右键那一行本身能不能直接
/// 交给 `ensure`。真正的归根不依赖它——交付单元名词表里本来就不可能出现
/// 这几个（服务端配置解析会把它们剔掉）。
const COARSE_HIERARCHY_NOUNS: [&str; 4] = ["WORL", "WORLD", "SITE", "ZONE"];

pub fn is_coarse_hierarchy_noun(noun: &str) -> bool {
    COARSE_HIERARCHY_NOUNS.contains(&noun.trim().to_ascii_uppercase().as_str())
}

/// 把「已经生产过模型的元素」归并成要交给 `ensure` 的生成根。
///
/// 三条来源并集去重，**都不依赖 `anc` 数组的顺序**：
///
/// 1. 元素祖先链上 noun 属于交付单元名词表的那些；
/// 2. 元素自身 noun 属于交付单元名词表的；
/// 3. 链上一个交付单元都没有的元素**自己**——那类元素（STRU/SCTN、FLOOR/WALL）
///    的生成根由服务端的 normal-root 策略解，客户端不抄那套 owner 链兜底。
///
/// 外加直管支管：`tubi_relate` 上的 BRAN / HANG 本身就是交付单元粒度，直接进。
///
/// 返回的是**上限**。嵌套交付单元（EQUI ⊃ SUPPO）与共根元素都会留在集合里，
/// 靠删除之后 `ensure(force = false)` 的 `AlreadyAvailable` 在服务端免费去重
/// ——客户端自己裁剪就要复制「哪个祖先更近」那套判定，那正是两份实现漂移的起点。
pub fn regeneration_roots(
    scope: &GeneratedScope,
    nouns: &HashMap<RefU64, String>,
    delivery_units: &HashSet<String>,
) -> Vec<RefU64> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |refno: RefU64, roots: &mut Vec<RefU64>| {
        if seen.insert(refno) {
            roots.push(refno);
        }
    };
    let is_unit = |refno: &RefU64| {
        nouns
            .get(refno)
            .is_some_and(|noun| delivery_units.contains(&noun.trim().to_ascii_uppercase()))
    };

    for element in &scope.elements {
        let refno = element.refno.refno();
        let mut covered = false;
        if is_unit(&refno) {
            push(refno, &mut roots);
            covered = true;
        }
        for ancestor in element.anc.iter().copied().map(RefU64) {
            if is_unit(&ancestor) {
                push(ancestor, &mut roots);
                covered = true;
            }
        }
        if !covered {
            push(refno, &mut roots);
        }
    }
    for bran in &scope.tubing_branches {
        push(*bran, &mut roots);
    }
    roots
}

/// 一个生成根跑完之后的去向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitOutcome {
    /// 真做了一趟，或者做完发现本来就没有可画几何——都算这一趟对它尽到了责任。
    Done,
    /// 同根的活刚被别的元素触发过（`AlreadyAvailable`），或者别的进程正占着
    /// 这个根（409），或者元素已经不在了（404），或者解不出根（412）。
    Skipped,
    /// 真失败。服务端留下 `RegenRoot` 的 failed 行，去「待重试单元」重试。
    Failed,
    /// 120 秒没等到，但后台还在跑。既不是成功也不是失败。
    Background,
    /// 整趟就此打住：再往下发只会扩大损失。
    Abort,
}

/// 成功回执的分档。`NoRenderableGeometry` 计入 `Done` 而不是失败——
/// 空的 BRAN、纯作层级用的 STRU 本来就没东西可画，重试一百遍还是同一个结果。
pub fn outcome_of_status(status: EnsureStatus) -> UnitOutcome {
    match status {
        EnsureStatus::Generated | EnsureStatus::NoRenderableGeometry | EnsureStatus::Unknown => {
            UnitOutcome::Done
        }
        EnsureStatus::AlreadyAvailable => UnitOutcome::Skipped,
    }
}

/// 失败回执的分档。
///
/// **`container` 必须响亮。** 我们已经归过根了，它一旦出现就说明客户端的候选根
/// 与服务端策略对不上（多半是 `/health` 的名词表没读到），静默跳过等于让这个
/// 分歧永远没人发现——那是宪法第三条点名的那类缺陷。
pub fn outcome_of_failure(failure: &Failure) -> UnitOutcome {
    match failure.code.as_str() {
        // 服务够不着 / 服务没准备好：删都删不成，继续发只是白费。
        "timeout" if failure.message.contains("按需生成超时") => UnitOutcome::Background,
        "timeout" => UnitOutcome::Abort,
        "initialization_not_ready" => UnitOutcome::Abort,
        // 这个根别人在做 / 元素没了 / 解不出根：这一个不做，下一个继续。
        "conflict" | "not_found" | "precondition" => UnitOutcome::Skipped,
        // 归根算错了。不许静默。
        "container" => UnitOutcome::Failed,
        _ => UnitOutcome::Failed,
    }
}

/// 一趟重新生成的账本。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub done: usize,
    pub skipped: usize,
    pub failed: usize,
    pub background: usize,
}

impl Tally {
    pub fn settled(&self) -> usize {
        self.done + self.skipped + self.failed + self.background
    }

    pub fn record(&mut self, outcome: UnitOutcome) {
        match outcome {
            UnitOutcome::Done => self.done += 1,
            UnitOutcome::Skipped => self.skipped += 1,
            UnitOutcome::Failed => self.failed += 1,
            UnitOutcome::Background => self.background += 1,
            // 中止的那一个不进账：它没跑完，也不会再跑。
            UnitOutcome::Abort => {}
        }
    }

    /// 收尾那句话。失败非零时点名去哪儿重试——那本账在服务端，前台不另存一份。
    pub fn summary(&self, total: usize) -> String {
        let mut line = format!(
            "{total} 个生成单元：成功 {}、跳过 {}、失败 {}",
            self.done, self.skipped, self.failed
        );
        if self.background > 0 {
            line.push_str(&format!("、仍在后台 {}", self.background));
        }
        if self.failed > 0 {
            line.push_str("。失败的去任务队列「待重试单元」重试");
        }
        if self.background > 0 {
            line.push_str("。后台那几个跑完后点一次取回工作才看得到");
        }
        line
    }
}

/// 确认框那句话。两个数字都来自真实查询，`roots` 标明是上限。
pub fn confirm_prompt(label: &str, elements: usize, roots: usize) -> String {
    format!(
        "将删除 {label} 范围内 {elements} 个已生成元素，归成最多 {roots} 个生成单元重做。\n\
         中途中断的话，没重做完的那些找不回来。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aios_core::rs_surreal::inst::GeneratedElement;

    fn units(nouns: &[&str]) -> HashSet<String> {
        nouns.iter().map(|n| n.to_string()).collect()
    }

    fn element(refno: u64, anc: &[u64]) -> GeneratedElement {
        GeneratedElement {
            refno: RefU64(refno).into(),
            anc: anc.to_vec(),
        }
    }

    fn nouns(pairs: &[(u64, &str)]) -> HashMap<RefU64, String> {
        pairs
            .iter()
            .map(|(refno, noun)| (RefU64(*refno), noun.to_string()))
            .collect()
    }

    /// 一根 BRAN 底下几十个管件归一个根：候选集里 BRAN 只出现一次，
    /// 那几十个件一个都不出现。这是整个特性成立的前提——按元素发 ensure
    /// 就是把同一根 BRAN 重生成几十遍。
    #[test]
    fn components_collapse_onto_their_delivery_unit() {
        let scope = GeneratedScope {
            elements: vec![
                element(101, &[100, 10, 1]),
                element(102, &[100, 10, 1]),
                element(103, &[100, 10, 1]),
            ],
            tubing_branches: Vec::new(),
        };
        let nouns = nouns(&[
            (1, "SITE"),
            (10, "PIPE"),
            (100, "BRAN"),
            (101, "ELBO"),
            (102, "FLAN"),
            (103, "VALV"),
        ]);

        let roots = regeneration_roots(&scope, &nouns, &units(&["BRAN", "HANG", "SUPPO", "EQUI"]));
        assert_eq!(roots, vec![RefU64(100)]);
    }

    /// `anc` 的顺序不作承诺，归根不许依赖它。把同一条链正着、倒着、乱序各来
    /// 一遍，候选集必须逐字相同。
    #[test]
    fn shuffled_ancestors_resolve_to_the_same_roots() {
        let nouns = nouns(&[(1, "SITE"), (10, "PIPE"), (100, "BRAN"), (101, "ELBO")]);
        let delivery = units(&["BRAN", "HANG", "SUPPO", "EQUI"]);

        let orders: [&[u64]; 3] = [&[100, 10, 1], &[1, 10, 100], &[10, 1, 100]];
        for anc in orders {
            let scope = GeneratedScope {
                elements: vec![element(101, anc)],
                tubing_branches: Vec::new(),
            };
            assert_eq!(
                regeneration_roots(&scope, &nouns, &delivery),
                vec![RefU64(100)],
                "anc 顺序 {anc:?} 改变了归根结果"
            );
        }
    }

    /// 链上一个交付单元都没有（STRU / SCTN 这类）：元素自己进候选，
    /// 由服务端的 normal-root 策略解根。客户端不抄那套 owner 链兜底。
    #[test]
    fn elements_without_a_delivery_unit_ancestor_go_in_as_themselves() {
        let scope = GeneratedScope {
            elements: vec![element(201, &[200, 1]), element(202, &[200, 1])],
            tubing_branches: Vec::new(),
        };
        let nouns = nouns(&[(1, "ZONE"), (200, "STRU"), (201, "SCTN"), (202, "SCTN")]);

        let roots = regeneration_roots(&scope, &nouns, &units(&["BRAN", "EQUI"]));
        assert_eq!(roots, vec![RefU64(201), RefU64(202)]);
    }

    /// 元素自身就是交付单元（右键一台 EQUI，它自己那行 inst_relate）：
    /// 进候选的是它自己，不是它的 ZONE。
    #[test]
    fn an_element_that_is_itself_a_delivery_unit_is_its_own_root() {
        let scope = GeneratedScope {
            elements: vec![element(300, &[3, 1])],
            tubing_branches: Vec::new(),
        };
        let nouns = nouns(&[(1, "SITE"), (3, "ZONE"), (300, "EQUI")]);

        let roots = regeneration_roots(&scope, &nouns, &units(&["EQUI"]));
        assert_eq!(roots, vec![RefU64(300)]);
    }

    /// 嵌套交付单元两个都留在候选里。裁剪需要「哪个更近」那套判定，
    /// 而那是服务端的规则——`force=false` 的 `AlreadyAvailable` 会把重复
    /// 挡在生成之前，代价只是一次几毫秒的往返。
    #[test]
    fn nested_delivery_units_both_stay_in_the_candidate_set() {
        let scope = GeneratedScope {
            elements: vec![element(401, &[400, 300, 3])],
            tubing_branches: Vec::new(),
        };
        let nouns = nouns(&[(3, "ZONE"), (300, "EQUI"), (400, "SUPPO"), (401, "BOX")]);

        let roots = regeneration_roots(&scope, &nouns, &units(&["EQUI", "SUPPO"]));
        assert_eq!(roots, vec![RefU64(400), RefU64(300)]);
    }

    /// 只有直管、没有管件的 BRAN 在 `inst_relate` 上一行都没有。漏掉
    /// `tubi_relate` 那一半，一整根光管的支管会被当成「没生成过」而跳过。
    #[test]
    fn tubing_only_branches_are_not_lost() {
        let scope = GeneratedScope {
            elements: Vec::new(),
            tubing_branches: vec![RefU64(500)],
        };

        let roots = regeneration_roots(&scope, &HashMap::new(), &units(&["BRAN"]));
        assert_eq!(roots, vec![RefU64(500)]);
        assert_eq!(scope.element_count(), 1, "确认框要把它数进去");
    }

    /// 名词表是项目配置，不是那四个默认值。项目把交付单元换成 `PIPE` 之后，
    /// 同一批元素必须归到 PIPE 而不是 BRAN——这正是硬编码会静默算错的那一步。
    #[test]
    fn the_delivery_unit_table_decides_the_granularity() {
        let scope = GeneratedScope {
            elements: vec![element(101, &[100, 10, 1])],
            tubing_branches: Vec::new(),
        };
        let nouns = nouns(&[(1, "SITE"), (10, "PIPE"), (100, "BRAN"), (101, "ELBO")]);

        assert_eq!(
            regeneration_roots(&scope, &nouns, &units(&["PIPE"])),
            vec![RefU64(10)]
        );
        assert_eq!(
            regeneration_roots(&scope, &nouns, &units(&["BRAN"])),
            vec![RefU64(100)]
        );
    }

    /// 粗层级名词永远当不了生成根。这张表只服务「右键这一行能不能直接发」，
    /// 归根路径不碰它。
    #[test]
    fn coarse_hierarchy_nouns_are_recognised_case_insensitively() {
        assert!(is_coarse_hierarchy_noun("ZONE"));
        assert!(is_coarse_hierarchy_noun(" site "));
        assert!(is_coarse_hierarchy_noun("WORLD"));
        assert!(!is_coarse_hierarchy_noun("BRAN"));
        assert!(!is_coarse_hierarchy_noun("STRU"));
    }

    /// 五种回执各归各的档。混成一个布尔就分不出「同根刚被做过」与「真做了」，
    /// 也分不出「本来就没几何」与「失败」。
    #[test]
    fn every_reply_lands_in_exactly_one_bucket() {
        assert_eq!(
            outcome_of_status(EnsureStatus::Generated),
            UnitOutcome::Done
        );
        assert_eq!(
            outcome_of_status(EnsureStatus::NoRenderableGeometry),
            UnitOutcome::Done
        );
        assert_eq!(
            outcome_of_status(EnsureStatus::AlreadyAvailable),
            UnitOutcome::Skipped
        );

        let skip = ["conflict", "not_found", "precondition"];
        for code in skip {
            assert_eq!(
                outcome_of_failure(&Failure::new(code, "")),
                UnitOutcome::Skipped,
                "{code}"
            );
        }
        assert_eq!(
            outcome_of_failure(&Failure::new("initialization_not_ready", "")),
            UnitOutcome::Abort
        );
        assert_eq!(
            outcome_of_failure(&Failure::new("timeout", "Connection refused")),
            UnitOutcome::Abort,
            "连不上就别再往下删了"
        );
        assert_eq!(
            outcome_of_failure(&Failure::new("timeout", "按需生成超时(120s)，后台继续执行")),
            UnitOutcome::Background,
            "服务端的 120 秒超时是「还在跑」，不是「够不着」"
        );
        assert_eq!(
            outcome_of_failure(&Failure::new("internal", "boom")),
            UnitOutcome::Failed
        );
    }

    /// 归根算错时唯一的信号就是这个 400。静默跳过它，两边的名词表可以错开
    /// 好几个月都没人知道。
    #[test]
    fn a_container_rejection_is_never_swallowed() {
        assert_eq!(
            outcome_of_failure(&Failure::new("container", "WORL/SITE/ZONE 不能做生成根")),
            UnitOutcome::Failed,
        );
    }

    /// 中止的那一个不进账——它没跑完，也不会再跑。把它记成失败会让人去
    /// 「待重试单元」找一行根本不存在的记录。
    #[test]
    fn the_aborting_unit_is_not_counted() {
        let mut tally = Tally::default();
        tally.record(UnitOutcome::Done);
        tally.record(UnitOutcome::Abort);
        assert_eq!(tally.settled(), 1);
        assert_eq!(tally.failed, 0);
    }

    /// 收尾那句话：有失败就必须指出去哪儿重试，有后台就必须说清还没完。
    #[test]
    fn the_summary_points_at_the_ledger_that_actually_holds_the_failures() {
        let clean = Tally {
            done: 3,
            skipped: 1,
            ..Default::default()
        };
        let line = clean.summary(4);
        assert!(line.contains("成功 3") && line.contains("跳过 1"), "{line}");
        assert!(!line.contains("待重试单元"), "没失败就别提重试: {line}");

        let messy = Tally {
            done: 1,
            failed: 2,
            background: 1,
            ..Default::default()
        };
        let line = messy.summary(4);
        assert!(line.contains("待重试单元"), "{line}");
        assert!(
            line.contains("取回工作"),
            "后台那几个要说清怎么看到: {line}"
        );
    }

    /// 确认框必须把「中断会丢」说出来。这是本特性接受「整片删一次」之后
    /// 唯一的补偿——`model_update` 那边也有一条同型的文案断言。
    #[test]
    fn the_confirm_prompt_says_an_interruption_loses_work() {
        let prompt = confirm_prompt("/ZONE-A", 12431, 340);
        assert!(
            prompt.contains("12431") && prompt.contains("340"),
            "{prompt}"
        );
        assert!(prompt.contains("找不回来"), "{prompt}");
    }
}
