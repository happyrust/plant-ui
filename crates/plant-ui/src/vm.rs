use aios_core::RefU64;

/// 主工作台视图模型（外壳字段随 M1 各视图逐步补齐）。
///
/// 外壳原则：宁可少一格，也不摆一个永远是 0 的假数字。所以 gen-model 侧口径的
/// 已应用 sesno / 文件 sesno / 待更新批次 / 待重试单元不进这里，等 M4-4 定下
/// 两仓库的数据边界后再补。
#[derive(Debug, Clone, Default)]
pub struct WorkbenchVm {
    /// 项目名（标题栏芯片 + 状态栏）。
    pub project: String,
    /// 库标识（如 "ns 1516 / db 7997"），等宽渲染。
    pub db: String,
    /// 当前用户（标题栏右侧）。
    pub user: String,
    /// 数据源是否就绪（状态栏指示点）。
    pub data_source_ok: bool,
    /// 已加载元素计数（状态栏右侧）。
    pub element_count: usize,
    /// 当前选中元素（状态栏；M1-6 起驱动属性视图跟随）。
    pub selected: Option<RefU64>,
}
