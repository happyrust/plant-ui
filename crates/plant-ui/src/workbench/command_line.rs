//! 命令交互视图：会话回显、单行输入与当前进程内的输入历史。

use egui::{Key, RichText, ScrollArea, TextEdit, Ui};

use crate::Cmd;
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens, space};
use crate::vm::{CommandLineKind, WorkbenchVm};

const HISTORY_CAP: usize = 100;

#[derive(Default)]
pub struct State {
    input: String,
    draft: String,
    history: Vec<String>,
    history_index: Option<usize>,
}

pub fn show(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    egui::Frame::new().fill(t.bg_panel).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.spacing_mut().item_spacing.y = 0.0;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .max_height((ui.available_height() - d.px(32.0)).max(0.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for line in &vm.command.lines {
                    let (prefix, color) = match line.kind {
                        CommandLineKind::Input => ("> ", t.accent),
                        CommandLineKind::Output => ("", t.text_primary),
                        CommandLineKind::Error => ("", t.danger),
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{prefix}{}", line.text))
                                .font(Font::mono(d))
                                .color(color),
                        )
                        .selectable(true),
                    );
                }
            });

        super::chrome::hairline(ui, t);
        egui::Frame::new()
            .fill(t.bg_header)
            .inner_margin(egui::Margin::symmetric(space::S2 as i8, 0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(d.px(31.0));
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new(">").font(Font::mono(d)).color(t.accent));
                    let response = ui.add(
                        TextEdit::singleline(&mut state.input)
                            .desired_width(f32::INFINITY)
                            .font(Font::mono(d))
                            .hint_text("输入 help 查看可用命令"),
                    );

                    if response.has_focus() && ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                        state.previous();
                    } else if response.has_focus() && ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                        state.next();
                    }

                    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        let command = state.input.trim().to_owned();
                        if !command.is_empty() {
                            state.remember(command.clone());
                            cmds.push(Cmd::SubmitCommand(command));
                        }
                        state.input.clear();
                        state.history_index = None;
                        state.draft.clear();
                        response.request_focus();
                    }
                });
            });
    });
}

impl State {
    fn remember(&mut self, command: String) {
        self.history.push(command);
        if self.history.len() > HISTORY_CAP {
            self.history.remove(0);
        }
    }

    fn previous(&mut self) {
        let Some(last) = self.history.len().checked_sub(1) else {
            return;
        };
        let index = match self.history_index {
            Some(index) => index.saturating_sub(1),
            None => {
                self.draft.clone_from(&self.input);
                last
            }
        };
        self.history_index = Some(index);
        self.input.clone_from(&self.history[index]);
    }

    fn next(&mut self) {
        let Some(index) = self.history_index else {
            return;
        };
        if index + 1 < self.history.len() {
            self.history_index = Some(index + 1);
            self.input.clone_from(&self.history[index + 1]);
        } else {
            self.history_index = None;
            self.input.clone_from(&self.draft);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::State;

    #[test]
    fn history_navigation_restores_the_unsubmitted_draft() {
        let mut state = State {
            input: "q NAME".into(),
            ..Default::default()
        };
        state.remember("help".into());
        state.remember("q TYPE".into());

        state.previous();
        assert_eq!(state.input, "q TYPE");
        state.previous();
        assert_eq!(state.input, "help");
        state.next();
        assert_eq!(state.input, "q TYPE");
        state.next();
        assert_eq!(state.input, "q NAME");
    }
}
