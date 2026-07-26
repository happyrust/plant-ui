//! plant-ui：绘制层（组件层 + 各屏纯绘制函数 + Vm 定义）。
//! 不认识数据库、不认识 Bevy：输入 &Vm，输出 Vec<Cmd>。

pub mod style;
pub mod vm;
pub mod workbench;

/// UI 发出的命令：独立应用直接执行，接进 Bevy 后转成 Event。
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    SelectElement(aios_core::RefU64),
    /// 展开 / 折叠模型树节点；子层未加载时由 App 侧懒加载。
    ToggleExpand(aios_core::RefU64),
    /// 属性面板提交了一次改值（回车或失焦）。`attr` 是真实属性名，
    /// `value` 是已按类型校验过的显示串。
    EditAttr {
        refno: aios_core::RefU64,
        attr: String,
        value: String,
    },
    /// 命令行错误行的处置入口：重做该行对应的操作（App 侧按行号找回原请求）。
    RetryLog(u64),
    /// 清空命令行缓冲。
    ClearConsole,
}
