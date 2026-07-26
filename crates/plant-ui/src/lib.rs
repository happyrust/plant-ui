//! plant-ui：绘制层（组件层 + 各屏纯绘制函数 + Vm 定义）。
//! 不认识数据库、不认识 Bevy：输入 &Vm，输出 Vec<Cmd>。
//! M0 骨架：占位面板只为验证依赖图与增量编译，组件层在 M0-6 迁入。

pub mod vm;

/// UI 发出的命令：独立应用直接执行，接进 Bevy 后转成 Event。
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    SelectElement(aios_core::RefU64),
}

/// M0 占位面板：证明 egui 0.35、egui-phosphor 与 aios_core 类型同处一个 crate。
pub fn workbench_placeholder(ui: &mut egui::Ui, vm: &vm::WorkbenchVm) -> Vec<Cmd> {
    let mut cmds = Vec::new();
    ui.heading("plant-ui / M0 skeleton");
    ui.label(format!("project: {}  elements: {}", vm.project, vm.element_count));
    if let Some(refno) = &vm.selected {
        ui.monospace(format!("selected: {refno:?}"));
        if ui
            .button(format!("{} re-select", egui_phosphor::regular::CURSOR_CLICK))
            .clicked()
        {
            cmds.push(Cmd::SelectElement(refno.clone()));
        }
    }
    cmds
}