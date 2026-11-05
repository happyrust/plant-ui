//! Spooling 面板（卷管 / 管段）的动作与纯逻辑。
//!
//! 与模型树分开成一块面板，理由见 `plant_ui_data::spooling` 的模块文档：
//! spool 不挂在 `SITE` 下，模型树那条 `SITE -> ZONE -> PIPE -> BRAN` 的链
//! 永远走不到它。这不是「树少了一个分支」，是**另一条层级**。
//!
//! 这一层只放动作枚举与不碰数据库的纯计算（分组切换时的行过滤、显示名拼装），
//! 查询在 `plant_ui_data::spooling`，状态在 `vm::SpoolingVm`。

/// Spooling 面板上能发起的动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpoolingAction {
    /// 第一次画出来（`SpoolingVm::Idle`）或失败态上的「重试」：拉分组统计。
    Load,
    /// 刷新钮：分组统计与当前分组的行一起重拉。
    Refresh,
    /// 点某个分组：拉它的行。
    SelectGroup(String),
    /// 点某一行：选中它（宿主把 refno 写进全局选择集，属性面板跟着走）。
    SelectRow(aios_core::RefU64),
    /// 行尾「定位」：在模型树上选中并展开到它，再让相机对准它（见 [`locate_cmds`]）。
    Locate(aios_core::RefU64),
    /// 点行内眼睛：让这一行名下的几何显示 / 隐藏。
    ///
    /// `visible` 是宿主算好的**下一次点击方向**（`SpoolingRowVm::next_visible`），
    /// 不是绘制层从图标反推的：图标跟的是三维实际渲染结果，指令还在路上时它不动。
    ///
    /// 这个变体只带 spool 自己的 refno，几何翻译由宿主完成——绘制层不知道
    /// 一个 spool 名下有哪些几何，也不该知道。
    SetVisible {
        refno: aios_core::RefU64,
        visible: bool,
    },
}

/// 「定位」展开成的宿主命令：先 `LocateElement`（选中、展开祖先、滚进视野），
/// 再 `Model(Focus)`（相机对准）。顺序不能反：取景只作用于主选中，先取景就对准了上一个选中。
///
/// spool 行本身（`PSPOOL` / `HSPOOL`）挂在 `PIPE -> PSLIST` 下，库要进模型树（MDB 声明或
/// `extra_element_dbnums`）才定位得到；它没有几何时相机不动，这是视口的既有语义。
pub fn locate_cmds(refno: aios_core::RefU64) -> Vec<crate::Cmd> {
    vec![
        crate::Cmd::LocateElement(refno),
        crate::Cmd::Model(crate::ModelAction::Focus(refno)),
    ]
}

/// 全 0 时空态下面那句「为什么」。
pub const EMPTY_HINT: &str =
    "带 SPLDRG / PSPOOL 的库要进 MDB 声明或写进 extra_element_dbnums，gen-model 才会解析";

/// 分组行尾的计数文案。
///
/// `0` 不写成「0 条」而写「无」：这一栏在 AMS 上六类全是 0（`/ALL` 的 `CURD`
/// 里没有带 spool 的库），写「无」至少不会让人以为「有 0 条这种怪事」。
/// 但它**不是空态**——分组本身还在，说明这一层是通的、只是这类数据没有。
pub fn group_count_text(count: usize) -> String {
    if count == 0 {
        "无".to_string()
    } else {
        format!("{count} 条")
    }
}

/// 一行 spool 的显示名。
///
/// `SPLDRG` / `SPOOL` / `PSPOOL` 都带名字；`PPIECE` 与部分老数据无名，
/// 那种行按 `noun 序号` 补——序号用 refno 的低位，不是数组下标：下标会随
/// 排序变化，同一个元素两次进列表显示成两个名字。
pub fn row_label(noun: &str, name: &str, refno: aios_core::RefU64) -> String {
    if !name.trim().is_empty() {
        return name.trim().to_string();
    }
    // `RefU64` 的 `Display` 是 `word0_word1`，取后一半就是 E3D 的元素号。
    let tail = refno.to_string();
    let tail = tail.rsplit('_').next().unwrap_or("?");
    format!("{noun} {tail}")
}

/// 行尾的元信息：`属主类型 · 库号 · 成员数`。
pub fn row_meta(owner_noun: &str, dbnum: Option<u32>, children_count: u32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !owner_noun.is_empty() {
        parts.push(owner_noun.to_string());
    }
    if let Some(dbnum) = dbnum {
        parts.push(format!("db {dbnum}"));
    }
    if children_count > 0 {
        parts.push(format!("{children_count} 成员"));
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aios_core::RefU64;

    #[test]
    fn zero_count_reads_as_none_not_zero_rows() {
        assert_eq!(group_count_text(0), "无");
        assert_eq!(group_count_text(73), "73 条");
    }

    /// 有名字就用名字，不拼 noun 前缀——`/403_LOS-001-PS01` 是车间认的号，
    /// 前面加个 `PSPOOL ` 反而没人对得上。
    #[test]
    fn named_rows_show_the_name_verbatim() {
        let refno = RefU64::from_two_nums(2013287639, 27);
        assert_eq!(
            row_label("PSPOOL", "/403_LOS-001-PS01", refno),
            "/403_LOS-001-PS01"
        );
    }

    /// 无名行按 refno 低位补序号，且**同一个 refno 恒得同一个名字**——
    /// 用下标会在排序变化时把名字换掉。
    #[test]
    fn unnamed_rows_use_refno_tail_so_the_name_is_stable() {
        let a = RefU64::from_two_nums(24381, 101407);
        let b = RefU64::from_two_nums(24381, 101408);
        assert_eq!(row_label("PPIECE", "", a), "PPIECE 101407");
        assert_eq!(row_label("PPIECE", "   ", a), row_label("PPIECE", "", a));
        assert_ne!(row_label("PPIECE", "", a), row_label("PPIECE", "", b));
    }

    /// 定位先选中再取景：取景只认主选中，反过来就对准了上一个元素。
    #[test]
    fn locate_selects_in_the_tree_before_focusing_the_camera() {
        let refno = RefU64::from_two_nums(2013287639, 27);
        assert_eq!(
            locate_cmds(refno),
            vec![
                crate::Cmd::LocateElement(refno),
                crate::Cmd::Model(crate::ModelAction::Focus(refno)),
            ]
        );
    }

    /// 空态提示要把两条出路都说出来：改 MDB，或者写补充名单。
    #[test]
    fn the_empty_hint_names_both_ways_in() {
        assert!(EMPTY_HINT.contains("MDB 声明"));
        assert!(EMPTY_HINT.contains("extra_element_dbnums"));
    }

    #[test]
    fn row_meta_skips_empty_parts() {
        assert_eq!(row_meta("", None, 0), "");
        assert_eq!(row_meta("PIPE", Some(7997), 3), "PIPE · db 7997 · 3 成员");
        assert_eq!(row_meta("SPLDRG", None, 2), "SPLDRG · 2 成员");
    }
}
