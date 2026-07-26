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
}
