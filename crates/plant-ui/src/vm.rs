use aios_core::RefU64;

/// 主工作台视图模型（M0 骨架版；字段随 M1 各视图逐步补齐）。
#[derive(Debug, Clone, Default)]
pub struct WorkbenchVm {
    pub project: String,
    pub element_count: usize,
    pub selected: Option<RefU64>,
}