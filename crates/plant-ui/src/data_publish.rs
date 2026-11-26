//! 三维数据发布任务窗。
//!
//! 本模块只维护表单与预览状态；真正的发布由宿主处理 `Cmd::SubmitDataPublish`。

use egui::{Align, ComboBox, CornerRadius, Layout, Margin, RichText, ScrollArea, Stroke, TextEdit};
use egui_extras::{Column, TableBuilder};

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, radius, space};
use crate::style::widgets;
use crate::vm::{PropsVm, TreeVm, WorkbenchVm};
use crate::{Cmd, RefU64};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PublishCategory {
    #[default]
    Process,
    Electrical,
    Instrumentation,
    Ventilation,
    Room,
}

impl PublishCategory {
    pub const ALL: [Self; 5] = [
        Self::Process,
        Self::Electrical,
        Self::Instrumentation,
        Self::Ventilation,
        Self::Room,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Process => "工艺专业-全量数据发布",
            Self::Electrical => "电气专业-全量数据发布",
            Self::Instrumentation => "仪控专业-全量数据发布",
            Self::Ventilation => "通风专业-全量数据发布",
            Self::Room => "设备房间位置信息发布",
        }
    }

    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Process => "/get_gy_bran_data",
            Self::Electrical => "/get_dq_bran_data",
            Self::Instrumentation => "/get_yk_bran_data",
            Self::Ventilation => "/get_tf_bran_data",
            // 设备房间位置发布走与工艺发布相同的数据中心契约。
            Self::Room => "/get_bran_data_room_code",
        }
    }

    /// 对应 `rs-server` `/api/metadata/previews` 的 `(discipline, target)`；不支持提取预览的类型为 `None`。
    pub fn preview_route(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Process => Some(("pipe", "bran")),
            Self::Electrical => Some(("elec", "bran")),
            Self::Instrumentation => Some(("inst", "bran")),
            Self::Ventilation => Some(("hvac", "bran")),
            Self::Room => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesignPhase {
    Preliminary,
    #[default]
    Detailed,
}

impl DesignPhase {
    pub const ALL: [Self; 2] = [Self::Preliminary, Self::Detailed];

    pub fn label(self) -> &'static str {
        match self {
            Self::Preliminary => "初设阶段",
            Self::Detailed => "施设阶段",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishElement {
    pub refno: RefU64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PublishRequest {
    pub title: String,
    pub category: PublishCategory,
    pub design_phase: DesignPhase,
    pub elements: Vec<PublishElement>,
    pub delete_refnos: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct DeletionCandidate {
    pub id: String,
    pub name: String,
    #[serde(skip, default = "default_checked")]
    pub selected: bool,
}

fn default_checked() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewAttributeRow {
    pub code: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewInstanceRow {
    pub object_model_code: String,
    pub instance_code: String,
    pub refno: Option<String>,
    pub operate: String,
    pub version: String,
    pub attributes: Vec<PreviewAttributeRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRelationRow {
    pub object_model_code: String,
    pub operate: String,
    pub version: String,
    pub from_summary: String,
    pub to_summary: String,
    pub attributes: Vec<PreviewAttributeRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSkippedRow {
    pub refno: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreviewPayloadLayer {
    pub project_code: String,
    pub owner: String,
    pub instances: Vec<PreviewInstanceRow>,
    pub relations: Vec<PreviewRelationRow>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PublishPreviewData {
    pub datacenter_layer: String,
    pub elapsed_ms: Option<u64>,
    pub datacenter: PreviewPayloadLayer,
    pub internal: PreviewPayloadLayer,
    pub skipped: Vec<PreviewSkippedRow>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewTab {
    #[default]
    Instances,
    Relations,
    Skipped,
    InputElements,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewLayerChoice {
    #[default]
    Datacenter,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

impl SortDir {
    fn toggle(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::Asc => " ▲",
            Self::Desc => " ▼",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceSortCol {
    ObjectCode,
    InstanceCode,
    Refno,
    Operate,
    Version,
    AttrCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationSortCol {
    ObjectCode,
    Operate,
    Version,
    FromSummary,
    ToSummary,
    AttrCount,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PreviewStatus {
    #[default]
    Idle,
    Loading,
    Ready(PublishPreviewData),
    Failed(String),
    Unsupported(String),
}

#[derive(Debug, Default)]
pub struct State {
    pub open: bool,
    pub preview_open: bool,
    pub preview_status: PreviewStatus,
    pub preview_request: Option<PublishRequest>,
    pub preview_tab: PreviewTab,
    pub preview_layer: PreviewLayerChoice,
    pub preview_filter: String,
    pub preview_object_filter: String,
    pub preview_selected_instance: Option<usize>,
    pub preview_instance_sort: Option<(InstanceSortCol, SortDir)>,
    pub preview_relation_filter: String,
    pub preview_relation_type_filter: String,
    pub preview_relation_sort: Option<(RelationSortCol, SortDir)>,
    pub preview_selected_relation: Option<usize>,
    pub preview_attr_filter: String,
    pub preview_skipped_filter: String,
    pub preview_copy_notice: Option<String>,
    pub delete_confirmation_open: bool,
    pub deletion_candidates: Vec<DeletionCandidate>,
    pub pending_request: Option<PublishRequest>,
    /// 从首次点击提交起，到宿主收到接口回执为止。
    pub submitting: bool,
    pub request: PublishRequest,
    pub selected_refno: Option<RefU64>,
}

impl State {
    pub fn start_preview(&mut self) -> Option<Cmd> {
        self.preview_open = true;
        self.preview_selected_instance = None;
        self.preview_instance_sort = None;
        self.preview_filter.clear();
        self.preview_object_filter.clear();
        self.preview_selected_relation = None;
        self.preview_relation_sort = None;
        self.preview_relation_filter.clear();
        self.preview_relation_type_filter.clear();
        self.preview_attr_filter.clear();
        self.preview_skipped_filter.clear();
        self.preview_copy_notice = None;
        let request = self.request.clone();
        self.preview_request = Some(request.clone());
        if request.elements.is_empty() {
            self.preview_status = PreviewStatus::Unsupported("请先添加至少一个提资元素".into());
            self.preview_tab = PreviewTab::InputElements;
            return None;
        }
        if request.category.preview_route().is_none() {
            self.preview_status = PreviewStatus::Unsupported(
                "当前提资类型暂不支持后端元数据提取预览，仅展示待提资元素".into(),
            );
            self.preview_tab = PreviewTab::InputElements;
            return None;
        }
        self.preview_status = PreviewStatus::Loading;
        self.preview_tab = PreviewTab::Instances;
        Some(Cmd::PreviewDataPublish(request))
    }

    /// 新入口「三维数据提资」（`data_handover`）也走这一步，所以对 crate 内可见。
    pub(crate) fn start_lookup(&mut self) -> Cmd {
        let request = self.request.clone();
        self.pending_request = Some(request.clone());
        self.submitting = true;
        self.delete_confirmation_open = false;
        self.deletion_candidates.clear();
        Cmd::LookupDataPublishDeletions(request)
    }

    fn confirm_publish(&mut self) -> Option<Cmd> {
        if self.submitting {
            return None;
        }
        let mut request = self.pending_request.clone()?;
        request.delete_refnos = self
            .deletion_candidates
            .iter()
            .filter(|candidate| candidate.selected)
            .map(|candidate| candidate.id.clone())
            .collect();
        self.submitting = true;
        Some(Cmd::SubmitDataPublish(request))
    }

    fn add_element(&mut self, element: PublishElement) -> bool {
        if self
            .request
            .elements
            .iter()
            .any(|existing| existing.refno == element.refno)
        {
            return false;
        }
        self.request.elements.push(element);
        true
    }

    fn remove_selected(&mut self) -> bool {
        let Some(selected_refno) = self.selected_refno.take() else {
            return false;
        };
        let original_len = self.request.elements.len();
        self.request
            .elements
            .retain(|element| element.refno != selected_refno);
        self.request.elements.len() != original_len
    }

    fn clear_elements(&mut self) {
        self.request.elements.clear();
        self.selected_refno = None;
    }
}

pub fn show(
    ctx: &egui::Context,
    tokens: &Tokens,
    density: Density,
    workbench: &WorkbenchVm,
    state: &mut State,
) -> Vec<Cmd> {
    if !state.open {
        state.preview_open = false;
        state.delete_confirmation_open = false;
        state.pending_request = None;
        return Vec::new();
    }

    let mut commands = Vec::new();
    let mut window_open = state.open;
    egui::Window::new("三维数据接口")
        .id(egui::Id::new("data-publish-window"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .open(&mut window_open)
        .collapsible(false)
        .resizable(false)
        .fixed_size([density.px(560.0), density.px(436.0)])
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
            let label_width = density.px(72.0);
            let field_width = (ui.available_width() - label_width - space::S3).max(density.px(360.0));

            egui::Grid::new("data-publish-form")
                .num_columns(2)
                .spacing([space::S3, space::S2])
                .show(ui, |ui| {
                    ui.spacing_mut().interact_size.y = density.input_h();
                    form_label(ui, tokens, density, label_width, "标题");
                    widgets::text_input(
                        ui,
                        tokens,
                        density,
                        egui::Id::new("data-publish-title"),
                        &mut state.request.title,
                        "请输入发布标题",
                        field_width,
                    );
                    ui.end_row();

                    form_label(ui, tokens, density, label_width, "提资内容");
                    ComboBox::from_id_salt("data-publish-category")
                        .selected_text(state.request.category.label())
                        .width(field_width)
                        .show_ui(ui, |ui| {
                            for category in PublishCategory::ALL {
                                ui.selectable_value(
                                    &mut state.request.category,
                                    category,
                                    category.label(),
                                );
                            }
                        });
                    ui.end_row();

                    form_label(ui, tokens, density, label_width, "设计阶段");
                    widgets::segmented(
                        ui,
                        tokens,
                        density,
                        &mut state.request.design_phase,
                        &[
                            (DesignPhase::Detailed, "施设阶段"),
                            (DesignPhase::Preliminary, "初设阶段"),
                        ],
                    );
                    ui.end_row();
                });

            ui.add_space(space::S1);
            ui.label(
                RichText::new("提资元素")
                    .font(Font::strong(density))
                    .color(tokens.text_primary),
            );

            let current_element = selected_workbench_element(workbench);
            let already_added = current_element.as_ref().is_some_and(|candidate| {
                state
                    .request
                    .elements
                    .iter()
                    .any(|element| element.refno == candidate.refno)
            });

            ui.horizontal_top(|ui| {
                ui.set_height(density.px(176.0));
                ui.vertical(|ui| {
                    ui.set_width(density.px(76.0));
                    let add_hint = if current_element.is_none() {
                        "请先在模型树或三维视图中选择元素"
                    } else {
                        "当前元素已经在提资列表中"
                    };
                    let add_response = ui
                        .add_enabled(
                            current_element.is_some() && !already_added,
                            widgets::button(tokens, density, "添加").full_width(),
                        )
                        .on_disabled_hover_text(add_hint);
                    if add_response.clicked()
                        && let Some(element) = current_element.clone()
                    {
                        state.add_element(element);
                    }

                    let remove_response = ui.add_enabled(
                        state.selected_refno.is_some(),
                        widgets::button(tokens, density, "移除").full_width(),
                    );
                    if remove_response.clicked() {
                        state.remove_selected();
                    }

                    let clear_response = ui.add_enabled(
                        !state.request.elements.is_empty(),
                        widgets::button(tokens, density, "清空").full_width(),
                    );
                    if clear_response.clicked() {
                        state.clear_elements();
                    }
                });

                egui::Frame::new()
                    .fill(tokens.bg_input)
                    .stroke(Stroke::new(1.0, tokens.border))
                    .corner_radius(CornerRadius::same(radius::MD))
                    .inner_margin(Margin::same(space::S2 as i8))
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(density.px(430.0), density.px(160.0)));
                        ScrollArea::vertical()
                            .id_salt("data-publish-elements")
                            .auto_shrink([false, false])
                            .max_height(density.px(160.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if state.request.elements.is_empty() {
                                    ui.centered_and_justified(|ui| {
                                        ui.label(
                                            RichText::new("尚未添加提资元素")
                                                .font(Font::meta(density))
                                                .color(tokens.text_muted),
                                        );
                                    });
                                    return;
                                }

                                for element in &state.request.elements {
                                    let selected = state.selected_refno == Some(element.refno);
                                    let response = ui.selectable_label(
                                        selected,
                                        format!("{}    {}", element.name, element.refno),
                                    );
                                    if response.clicked() {
                                        state.selected_refno = Some(element.refno);
                                    }
                                }
                            });
                    });
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let can_submit = !state.submitting
                    && !state.delete_confirmation_open
                    && !state.request.elements.is_empty();
                let submit = ui.add_enabled(
                    can_submit,
                    widgets::button(tokens, density, "提交")
                        .primary()
                        .loading(state.submitting),
                );
                if !can_submit {
                    submit
                        .clone()
                        .on_disabled_hover_text(if state.delete_confirmation_open {
                            "请先关闭确认删除提资窗口"
                        } else if state.submitting {
                            "正在提交，请稍候"
                        } else {
                            "请至少添加一个元素"
                        });
                }
                if submit.clicked() {
                    commands.push(state.start_lookup());
                }
                if ui.add(widgets::button(tokens, density, "预览")).clicked()
                    && let Some(cmd) = state.start_preview()
                {
                    commands.push(cmd);
                }
            });
        });

    state.open = window_open || (state.submitting && state.delete_confirmation_open);
    if !state.open {
        state.preview_open = false;
        state.delete_confirmation_open = false;
        state.pending_request = None;
    } else if state.preview_open {
        if let Some(cmd) = show_preview(ctx, tokens, density, state) {
            commands.push(cmd);
        }
    }

    if state.open && state.delete_confirmation_open {
        if let Some(command) = show_delete_confirmation(ctx, tokens, density, state) {
            commands.push(command);
        }
    }

    commands
}

/// 确认删除提资子窗。新入口「三维数据提资」复用这一段，所以对 crate 内可见。
pub(crate) fn show_delete_confirmation(
    ctx: &egui::Context,
    tokens: &Tokens,
    density: Density,
    state: &mut State,
) -> Option<Cmd> {
    let mut open = state.delete_confirmation_open;
    let mut close_requested = false;
    let mut confirmed = false;
    egui::Window::new("确认删除提资")
        .id(egui::Id::new("data-publish-delete-confirmation"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size([density.px(420.0), density.px(300.0)])
        .show(ctx, |ui| {
            ui.add_enabled_ui(!state.submitting, |ui| {
                let widgets = &mut ui.visuals_mut().widgets;
                for visuals in [
                    &mut widgets.inactive,
                    &mut widgets.hovered,
                    &mut widgets.active,
                ] {
                    visuals.corner_radius = CornerRadius::ZERO;
                }

                let any_selected = state.deletion_candidates.iter().any(|item| item.selected);
                let mut all_selected = !state.deletion_candidates.is_empty()
                    && state.deletion_candidates.iter().all(|item| item.selected);
                let partially_selected = any_selected && !all_selected;
                if ui
                    .add(
                        egui::Checkbox::new(&mut all_selected, "全选")
                            .indeterminate(partially_selected),
                    )
                    .changed()
                {
                    for candidate in &mut state.deletion_candidates {
                        candidate.selected = all_selected;
                    }
                }

                egui::Frame::new()
                    .fill(tokens.bg_input)
                    .stroke(Stroke::new(1.0, tokens.border))
                    .corner_radius(CornerRadius::same(radius::MD))
                    .inner_margin(Margin::same(space::S2 as i8))
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(density.px(375.0), density.px(160.0)));
                        ScrollArea::vertical()
                            .id_salt("data-publish-delete-candidates")
                            .auto_shrink([false, false])
                            .max_height(density.px(160.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if state.deletion_candidates.is_empty() {
                                    ui.label("没有待删除的提资");
                                }
                                for candidate in &mut state.deletion_candidates {
                                    ui.horizontal_top(|ui| {
                                        ui.add(egui::Checkbox::without_text(
                                            &mut candidate.selected,
                                        ));
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&candidate.name)
                                                    .font(Font::body(density))
                                                    .color(tokens.text_secondary),
                                            )
                                            .wrap(),
                                        );
                                    });
                                }
                            });
                    });
            });
            ui.add_space((ui.available_height() - density.btn_h()).max(0.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let confirm = ui.add_enabled(
                    !state.submitting,
                    widgets::button(tokens, density, "确认")
                        .primary()
                        .loading(state.submitting),
                );
                if confirm.clicked() {
                    confirmed = true;
                }
                if ui
                    .add_enabled(!state.submitting, widgets::button(tokens, density, "取消"))
                    .clicked()
                {
                    close_requested = true;
                }
            });
        });
    state.delete_confirmation_open = state.submitting || (open && !close_requested);
    if confirmed {
        return state.confirm_publish();
    }
    if !state.delete_confirmation_open {
        state.pending_request = None;
    }
    None
}

fn form_label(ui: &mut egui::Ui, tokens: &Tokens, density: Density, width: f32, text: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, density.input_h()),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.label(
                RichText::new(text)
                    .font(Font::label(density))
                    .color(tokens.text_secondary),
            );
        },
    );
}

pub(crate) fn selected_workbench_element(workbench: &WorkbenchVm) -> Option<PublishElement> {
    let refno = workbench.selection.primary()?;
    let name_from_props = match &workbench.props {
        PropsVm::Ready(props) if props.refno == refno && !props.name.trim().is_empty() => {
            Some(props.name.clone())
        }
        _ => None,
    };
    let name_from_tree = match &workbench.tree {
        TreeVm::Ready(rows) => rows
            .iter()
            .find(|row| row.refno == refno)
            .map(|row| row.name.clone()),
        TreeVm::Loading | TreeVm::Failed(_) => None,
    };
    Some(PublishElement {
        refno,
        name: name_from_props
            .or(name_from_tree)
            .unwrap_or_else(|| refno.to_string()),
    })
}

fn split_preview_heights(avail_h: f32, has_detail: bool, density: Density) -> (f32, f32) {
    if !has_detail {
        return ((avail_h - density.px(8.0)).max(density.px(100.0)), 0.0);
    }
    let detail_total = (avail_h * 0.36).clamp(density.px(116.0), density.px(176.0));
    let detail_table_h = (detail_total - density.px(32.0)).max(density.px(72.0));
    let main_table_h = (avail_h - detail_total - density.px(10.0)).max(density.px(96.0));
    (main_table_h, detail_table_h)
}

/// 预览提取数据子窗。新入口「三维数据提资」复用这一段，所以对 crate 内可见。
pub(crate) fn show_preview(
    ctx: &egui::Context,
    tokens: &Tokens,
    density: Density,
    state: &mut State,
) -> Option<Cmd> {
    let mut open = state.preview_open;
    let mut close_requested = false;
    let mut retry_cmd = None;
    let request = state
        .preview_request
        .clone()
        .unwrap_or_else(|| state.request.clone());

    let viewport = ctx.input(|i| i.viewport_rect());
    let margin = density.px(24.0);
    let max_w = (viewport.width() - margin).max(density.px(520.0));
    let max_h = (viewport.height() - margin).max(density.px(360.0));
    let default_w = density.px(900.0).min(max_w);
    let default_h = density.px(580.0).min(max_h);
    let min_w = density.px(520.0).min(max_w);
    let min_h = density.px(360.0).min(max_h);
    let default_pos = egui::pos2(
        viewport.left() + ((viewport.width() - default_w) * 0.5).max(density.px(12.0)),
        viewport.top() + ((viewport.height() - default_h) * 0.5).max(density.px(12.0)),
    );
    if ctx.input(|i| i.key_pressed(egui::Key::Escape))
        && ctx.memory(|m| m.focused().is_none())
    {
        close_requested = true;
    }

    egui::Window::new("预览提取数据")
        .id(egui::Id::new("data-publish-preview-window"))
        .open(&mut open)
        .movable(true)
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .default_pos(default_pos)
        .default_size([default_w, default_h])
        .min_size([min_w, min_h])
        .max_size([max_w, max_h])
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);

            let is_loading = matches!(state.preview_status, PreviewStatus::Loading);
            // 顶部概要条
            ui.horizontal_wrapped(|ui| {
                ui.add(widgets::status_tag(
                    tokens,
                    density,
                    request.category.label(),
                    Status::Info,
                ));
                let title_text = if request.title.trim().is_empty() {
                    "未命名提资".to_owned()
                } else {
                    format!("标题：{}", request.title.trim())
                };
                preview_value(ui, tokens, density, &title_text);
                preview_label(
                    ui,
                    tokens,
                    density,
                    &format!("· {} · 已选 {} 个元素", request.design_phase.label(), request.elements.len()),
                );

                if let PreviewStatus::Ready(data) = &state.preview_status {
                    let layer = preview_active_layer(data, state.preview_layer);
                    if !layer.project_code.is_empty() {
                        preview_label(ui, tokens, density, &format!("· 项目号 {}", layer.project_code));
                    }
                    if let Some(ms) = data.elapsed_ms {
                        preview_label(ui, tokens, density, &format!("· 耗时 {ms} ms"));
                    }
                    if data.datacenter_layer == "datacenter" {
                        ui.separator();
                        if ui
                            .selectable_label(
                                state.preview_layer == PreviewLayerChoice::Datacenter,
                                RichText::new("数据中心编码").font(Font::micro(density)),
                            )
                            .clicked()
                        {
                            state.preview_layer = PreviewLayerChoice::Datacenter;
                            state.preview_selected_instance = (!data.datacenter.instances.is_empty()).then_some(0);
                            state.preview_selected_relation = (!data.datacenter.relations.is_empty()).then_some(0);
                            state.preview_attr_filter.clear();
                        }
                        if ui
                            .selectable_label(
                                state.preview_layer == PreviewLayerChoice::Internal,
                                RichText::new("内部编码").font(Font::micro(density)),
                            )
                            .clicked()
                        {
                            state.preview_layer = PreviewLayerChoice::Internal;
                            state.preview_selected_instance = (!data.internal.instances.is_empty()).then_some(0);
                            state.preview_selected_relation = (!data.internal.relations.is_empty()).then_some(0);
                            state.preview_attr_filter.clear();
                        }
                    }
                }
            });

            // 页签栏 + 重新提取
            let (inst_count, rel_count, skip_count, warnings) = match &state.preview_status {
                PreviewStatus::Ready(data) => {
                    let layer = preview_active_layer(data, state.preview_layer);
                    (
                        layer.instances.len(),
                        layer.relations.len(),
                        data.skipped.len(),
                        data.warnings.clone(),
                    )
                }
                _ => (0, 0, 0, Vec::new()),
            };
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (PreviewTab::Instances, format!("提取实例 ({inst_count})")),
                    (PreviewTab::Relations, format!("关联关系 ({rel_count})")),
                    (PreviewTab::Skipped, format!("跳过节点 ({skip_count})")),
                    (
                        PreviewTab::InputElements,
                        format!("待提资元素 ({})", request.elements.len()),
                    ),
                ] {
                    if ui
                        .selectable_label(
                            state.preview_tab == tab,
                            RichText::new(label).font(Font::label(density)),
                        )
                        .clicked()
                    {
                        state.preview_tab = tab;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(widgets::button(tokens, density, "关闭")).clicked() {
                        close_requested = true;
                    }
                    let refresh_label = if is_loading { "提取中…" } else { "重新提取" };
                    if ui
                        .add_enabled(
                            !is_loading,
                            widgets::button(tokens, density, refresh_label).loading(is_loading),
                        )
                        .clicked()
                    {
                        retry_cmd = state.start_preview();
                    }
                });
            });

            if !warnings.is_empty() {
                egui::Frame::new()
                    .fill(tokens.warn_bg)
                    .stroke(Stroke::new(1.0, tokens.warn))
                    .corner_radius(CornerRadius::same(radius::MD))
                    .inner_margin(Margin::same(space::S2 as i8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        for warning in &warnings {
                            ui.label(
                                RichText::new(format!("提示：{warning}"))
                                    .font(Font::micro(density))
                                    .color(tokens.warn),
                            );
                        }
                    });
            }

            ui.separator();

            if state.preview_tab == PreviewTab::InputElements {
                preview_input_elements_tab(
                    ui,
                    tokens,
                    density,
                    &request,
                    &mut state.preview_copy_notice,
                );
                return;
            }

            match &state.preview_status {
                PreviewStatus::Idle => {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new("点击「重新提取」以获取后端元数据")
                                .font(Font::meta(density))
                                .color(tokens.text_muted),
                        );
                    });
                }
                PreviewStatus::Loading => {
                    ui.centered_and_justified(|ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                RichText::new("正在向数据服务提取元数据，请稍候…")
                                    .font(Font::body(density))
                                    .color(tokens.text_secondary),
                            );
                        });
                    });
                }
                PreviewStatus::Failed(err) => {
                    let message = err.clone();
                    egui::Frame::new()
                        .fill(tokens.danger_bg)
                        .stroke(Stroke::new(1.0, tokens.danger))
                        .corner_radius(CornerRadius::same(radius::MD))
                        .inner_margin(Margin::same(space::S3 as i8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new("提取预览数据失败")
                                    .font(Font::strong(density))
                                    .color(tokens.danger),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&message)
                                        .font(Font::body(density))
                                        .color(tokens.text_primary),
                                )
                                .wrap(),
                            );
                            ui.add_space(space::S2);
                            if ui.add(widgets::button(tokens, density, "重试")).clicked() {
                                retry_cmd = state.start_preview();
                            }
                        });
                    ui.add_space(space::S2);
                    preview_input_elements_tab(
                        ui,
                        tokens,
                        density,
                        &request,
                        &mut state.preview_copy_notice,
                    );
                }
                PreviewStatus::Unsupported(reason) => {
                    let reason = reason.clone();
                    egui::Frame::new()
                        .fill(tokens.bg_input)
                        .stroke(Stroke::new(1.0, tokens.border))
                        .corner_radius(CornerRadius::same(radius::MD))
                        .inner_margin(Margin::same(space::S2 as i8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new(&reason)
                                    .font(Font::body(density))
                                    .color(tokens.text_secondary),
                            );
                        });
                    ui.add_space(space::S2);
                    preview_input_elements_tab(
                        ui,
                        tokens,
                        density,
                        &request,
                        &mut state.preview_copy_notice,
                    );
                }
                PreviewStatus::Ready(data) => {
                    let data = data.clone();
                    let layer = preview_active_layer(&data, state.preview_layer);
                    match state.preview_tab {
                        PreviewTab::Instances => {
                            preview_instances_tab(ui, tokens, density, layer, state);
                        }
                        PreviewTab::Relations => {
                            preview_relations_tab(ui, tokens, density, layer, state);
                        }
                        PreviewTab::Skipped => {
                            preview_skipped_tab(ui, tokens, density, &data.skipped, state);
                        }
                        PreviewTab::InputElements => {
                            preview_input_elements_tab(
                                ui,
                                tokens,
                                density,
                                &request,
                                &mut state.preview_copy_notice,
                            );
                        }
                    }
                }
            }
        });

    state.preview_open = open && !close_requested;
    retry_cmd
}

fn preview_active_layer<'a>(
    data: &'a PublishPreviewData,
    choice: PreviewLayerChoice,
) -> &'a PreviewPayloadLayer {
    match choice {
        PreviewLayerChoice::Datacenter => &data.datacenter,
        PreviewLayerChoice::Internal => &data.internal,
    }
}

fn summarize_attributes(attrs: &[PreviewAttributeRow], max_items: usize) -> String {
    attrs
        .iter()
        .take(max_items)
        .map(|a| {
            let val = if a.value.chars().count() > 24 {
                let short: String = a.value.chars().take(24).collect();
                format!("{short}…")
            } else {
                a.value.clone()
            };
            format!("{}={}", a.code, val)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn format_attributes_tooltip(attrs: &[PreviewAttributeRow]) -> String {
    attrs
        .iter()
        .map(|a| format!("{} = {}", a.code, a.value))
        .collect::<Vec<_>>()
        .join("\n")
}

fn instance_matches_filter(row: &PreviewInstanceRow, q: &str, obj_filter: &str) -> bool {
    if !obj_filter.is_empty() && row.object_model_code != obj_filter {
        return false;
    }
    if q.is_empty() {
        return true;
    }
    let q_slash = q.replace('_', "/");
    row.object_model_code.to_lowercase().contains(q)
        || row.instance_code.to_lowercase().contains(q)
        || row
            .refno
            .as_deref()
            .is_some_and(|r| r.to_lowercase().contains(q) || r.to_lowercase().contains(&q_slash))
        || row.attributes.iter().any(|a| {
            a.code.to_lowercase().contains(q) || a.value.to_lowercase().contains(q)
        })
}

fn relation_matches_filter(row: &PreviewRelationRow, q: &str, type_filter: &str) -> bool {
    if !type_filter.is_empty() && row.object_model_code != type_filter {
        return false;
    }
    if q.is_empty() {
        return true;
    }
    let q_slash = q.replace('_', "/");
    row.object_model_code.to_lowercase().contains(q)
        || row.operate.to_lowercase().contains(q)
        || row.version.to_lowercase().contains(q)
        || row.from_summary.to_lowercase().contains(q)
        || row.from_summary.to_lowercase().contains(&q_slash)
        || row.to_summary.to_lowercase().contains(q)
        || row.to_summary.to_lowercase().contains(&q_slash)
        || row.attributes.iter().any(|a| {
            a.code.to_lowercase().contains(q)
                || a.value.to_lowercase().contains(q)
                || a.value.to_lowercase().contains(&q_slash)
        })
}

fn skipped_matches_filter(row: &PreviewSkippedRow, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    let q_slash = q.replace('_', "/");
    row.refno.to_lowercase().contains(q)
        || row.refno.to_lowercase().contains(&q_slash)
        || row.reason.to_lowercase().contains(q)
}

fn cmp_str(a: &str, b: &str, dir: SortDir) -> std::cmp::Ordering {
    let ord = a.cmp(b);
    match dir {
        SortDir::Asc => ord,
        SortDir::Desc => ord.reverse(),
    }
}

fn sort_filtered_instances(
    rows: &mut [(usize, &PreviewInstanceRow)],
    sort: Option<(InstanceSortCol, SortDir)>,
) {
    let Some((col, dir)) = sort else {
        return;
    };
    rows.sort_by(|(idx_a, a), (idx_b, b)| {
        let primary = match col {
            InstanceSortCol::ObjectCode => cmp_str(&a.object_model_code, &b.object_model_code, dir),
            InstanceSortCol::InstanceCode => cmp_str(&a.instance_code, &b.instance_code, dir),
            InstanceSortCol::Refno => {
                cmp_str(a.refno.as_deref().unwrap_or(""), b.refno.as_deref().unwrap_or(""), dir)
            }
            InstanceSortCol::Operate => cmp_str(&a.operate, &b.operate, dir),
            InstanceSortCol::Version => cmp_str(&a.version, &b.version, dir),
            InstanceSortCol::AttrCount => {
                let ord = a.attributes.len().cmp(&b.attributes.len());
                match dir {
                    SortDir::Asc => ord,
                    SortDir::Desc => ord.reverse(),
                }
            }
        };
        primary.then_with(|| idx_a.cmp(idx_b))
    });
}

fn sort_filtered_relations(
    rows: &mut [(usize, &PreviewRelationRow)],
    sort: Option<(RelationSortCol, SortDir)>,
) {
    let Some((col, dir)) = sort else {
        return;
    };
    rows.sort_by(|(idx_a, a), (idx_b, b)| {
        let primary = match col {
            RelationSortCol::ObjectCode => cmp_str(&a.object_model_code, &b.object_model_code, dir),
            RelationSortCol::Operate => cmp_str(&a.operate, &b.operate, dir),
            RelationSortCol::Version => cmp_str(&a.version, &b.version, dir),
            RelationSortCol::FromSummary => cmp_str(&a.from_summary, &b.from_summary, dir),
            RelationSortCol::ToSummary => cmp_str(&a.to_summary, &b.to_summary, dir),
            RelationSortCol::AttrCount => {
                let ord = a.attributes.len().cmp(&b.attributes.len());
                match dir {
                    SortDir::Asc => ord,
                    SortDir::Desc => ord.reverse(),
                }
            }
        };
        primary.then_with(|| idx_a.cmp(idx_b))
    });
}

fn toggle_sort<C: PartialEq + Copy>(current: &mut Option<(C, SortDir)>, col: C) {
    *current = match *current {
        Some((prev, dir)) if prev == col => {
            if dir == SortDir::Desc {
                None
            } else {
                Some((col, dir.toggle()))
            }
        }
        _ => Some((col, SortDir::Asc)),
    };
}

fn tsv_cell(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ")
}

fn format_instances_tsv(rows: &[(usize, &PreviewInstanceRow)]) -> String {
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push("#\t对象类编码\t实例编码\t来源参考号\t操作\t版本\t属性数\t关键属性摘要".to_owned());
    for (idx, (_, row)) in rows.iter().enumerate() {
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            idx + 1,
            tsv_cell(&row.object_model_code),
            tsv_cell(&row.instance_code),
            tsv_cell(row.refno.as_deref().unwrap_or("")),
            tsv_cell(&row.operate),
            tsv_cell(&row.version),
            row.attributes.len(),
            tsv_cell(&summarize_attributes(&row.attributes, 8)),
        ));
    }
    lines.join("\n")
}

fn format_relations_tsv(rows: &[(usize, &PreviewRelationRow)]) -> String {
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push("#\t关系类编码\t操作\t版本\t起点 / 主对象\t终点 / 关联对象\t属性数\t属性明细".to_owned());
    for (idx, (_, rel)) in rows.iter().enumerate() {
        let attrs = rel
            .attributes
            .iter()
            .map(|a| format!("{}={}", a.code, a.value))
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            idx + 1,
            tsv_cell(&rel.object_model_code),
            tsv_cell(&rel.operate),
            tsv_cell(&rel.version),
            tsv_cell(&rel.from_summary),
            tsv_cell(&rel.to_summary),
            rel.attributes.len(),
            tsv_cell(&attrs),
        ));
    }
    lines.join("\n")
}

fn format_attributes_tsv(attrs: &[PreviewAttributeRow]) -> String {
    let mut lines = Vec::with_capacity(attrs.len() + 1);
    lines.push("属性编码\t属性值".to_owned());
    for attr in attrs {
        lines.push(format!("{}\t{}", tsv_cell(&attr.code), tsv_cell(&attr.value)));
    }
    lines.join("\n")
}

fn format_skipped_tsv(rows: &[(usize, &PreviewSkippedRow)]) -> String {
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push("#\t参考号\t跳过原因".to_owned());
    for (idx, (_, item)) in rows.iter().enumerate() {
        lines.push(format!(
            "{}\t{}\t{}",
            idx + 1,
            tsv_cell(&item.refno),
            tsv_cell(&item.reason),
        ));
    }
    lines.join("\n")
}

fn format_elements_tsv(elements: &[PublishElement]) -> String {
    let mut lines = Vec::with_capacity(elements.len() + 1);
    lines.push("#\t元素名称\t参考号".to_owned());
    for (idx, el) in elements.iter().enumerate() {
        lines.push(format!(
            "{}\t{}\t{}",
            idx + 1,
            tsv_cell(&el.name),
            el.refno,
        ));
    }
    lines.join("\n")
}

fn paint_table_header_cell(ui: &mut egui::Ui, tokens: &Tokens) {
    let rect = ui.max_rect();
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, tokens.bg_header);
    ui.painter().line_segment(
        [
            egui::pos2(rect.left(), rect.bottom()),
            egui::pos2(rect.right(), rect.bottom()),
        ],
        Stroke::new(1.0, tokens.border),
    );
}

fn static_header_cell(ui: &mut egui::Ui, tokens: &Tokens, density: Density, title: &str) {
    paint_table_header_cell(ui, tokens);
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.add_space(space::S1);
        ui.label(
            RichText::new(title)
                .font(Font::strong(density))
                .color(tokens.text_primary),
        );
    });
}

fn sortable_header_cell(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    density: Density,
    title: &str,
    active_dir: Option<SortDir>,
) -> bool {
    paint_table_header_cell(ui, tokens);
    let mut clicked = false;
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.add_space(space::S1);
        let label_text = match active_dir {
            Some(dir) => format!("{title}{}", dir.marker()),
            None => title.to_owned(),
        };
        let color = if active_dir.is_some() {
            tokens.accent
        } else {
            tokens.text_primary
        };
        let resp = ui
            .add(
                egui::Label::new(
                    RichText::new(label_text)
                        .font(Font::strong(density))
                        .color(color),
                )
                .sense(egui::Sense::click()),
            )
            .on_hover_text("点击按此列排序（升序 / 降序 / 还原）");
        clicked = resp.clicked();
    });
    clicked
}

fn configure_selectable_table_style(ui: &mut egui::Ui, tokens: &Tokens) {
    ui.style_mut().interaction.selectable_labels = false;
    let visuals = ui.visuals_mut();
    visuals.selection.bg_fill = tokens.accent;
    visuals.selection.stroke = Stroke::new(1.0, tokens.accent_ink);
    visuals.widgets.hovered.bg_fill = tokens.accent_bg;
}

fn row_text_colors(tokens: &Tokens, selected: bool) -> (egui::Color32, egui::Color32, egui::Color32) {
    if selected {
        (
            tokens.accent_ink,
            tokens.accent_ink.gamma_multiply(0.90),
            tokens.accent_ink.gamma_multiply(0.80),
        )
    } else {
        (tokens.text_primary, tokens.text_secondary, tokens.text_muted)
    }
}

fn truncated_cell(
    ui: &mut egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    hover_text: Option<&str>,
) {
    let display = if text.trim().is_empty() { "—" } else { text };
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.add_space(space::S1);
        let resp = ui.add(
            egui::Label::new(RichText::new(display).font(font).color(color))
                .truncate()
                .selectable(false)
                .sense(egui::Sense::hover()),
        );
        if let Some(tip) = hover_text
            && !tip.trim().is_empty()
        {
            resp.on_hover_text(tip);
        }
    });
}

fn search_clear_button(ui: &mut egui::Ui, tokens: &Tokens, density: Density, query: &mut String) {
    if !query.is_empty()
        && ui
            .add(widgets::button(tokens, density, "×"))
            .on_hover_text("清空筛选")
            .clicked()
    {
        query.clear();
    }
}

fn copy_notice_pill(ui: &mut egui::Ui, tokens: &Tokens, density: Density, notice: &Option<String>) {
    if let Some(msg) = notice {
        ui.add(widgets::status_tag(tokens, density, msg, Status::Success));
    }
}

fn render_attribute_detail_panel(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    density: Density,
    title: &str,
    attrs: &[PreviewAttributeRow],
    attr_filter: &mut String,
    copy_notice: &mut Option<String>,
    id_salt: &str,
    detail_table_h: f32,
) {
    let q = attr_filter.trim().to_lowercase();
    let filtered_attrs: Vec<&PreviewAttributeRow> = attrs
        .iter()
        .filter(|a| {
            q.is_empty()
                || a.code.to_lowercase().contains(&q)
                || a.value.to_lowercase().contains(&q)
        })
        .collect();

    ui.add_space(space::S1);
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!(
                "属性明细 — {title}（{} / {} 项）",
                filtered_attrs.len(),
                attrs.len()
            ))
            .font(Font::strong(density))
            .color(tokens.text_primary),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(!filtered_attrs.is_empty(), widgets::button(tokens, density, "复制全部属性"))
                .clicked()
            {
                let copied: Vec<PreviewAttributeRow> =
                    filtered_attrs.iter().map(|a| (*a).clone()).collect();
                ui.ctx().copy_text(format_attributes_tsv(&copied));
                *copy_notice = Some(format!("已复制 {} 项属性", copied.len()));
            }
            search_clear_button(ui, tokens, density, attr_filter);
            ui.add(
                TextEdit::singleline(attr_filter)
                    .desired_width(density.px(160.0))
                    .hint_text("筛选属性编码或值…"),
            );
        });
    });

    egui::Frame::new()
        .fill(tokens.bg_panel)
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::same(space::S1 as i8))
        .show(ui, |ui| {
            let scroll_h = (detail_table_h - density.px(4.0)).max(density.px(56.0));
            if filtered_attrs.is_empty() {
                ui.set_height(scroll_h.min(density.px(80.0)));
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("没有匹配的属性项")
                            .font(Font::meta(density))
                            .color(tokens.text_muted),
                    );
                });
                return;
            }
            let row_h = density.px(22.0);
            let header_h = density.px(24.0);
            ui.allocate_ui(egui::vec2(ui.available_width(), detail_table_h), |ui| {
                ui.push_id(id_salt, |ui| {
                    TableBuilder::new(ui)
                        .striped(true)
                        .resizable(true)
                        .vscroll(true)
                        .min_scrolled_height(0.0)
                        .max_scroll_height(scroll_h)
                        .cell_layout(Layout::left_to_right(Align::Center))
                        .column(Column::initial(density.px(170.0)).at_least(density.px(100.0)))
                        .column(Column::remainder().at_least(density.px(180.0)))
                        .header(header_h, |mut header| {
                            header.col(|ui| static_header_cell(ui, tokens, density, "属性编码"));
                            header.col(|ui| {
                                static_header_cell(ui, tokens, density, "属性值（点击可复制）")
                            });
                        })
                        .body(|body| {
                            body.rows(row_h, filtered_attrs.len(), |mut row| {
                                let attr = filtered_attrs[row.index()];
                                row.col(|ui| {
                                    truncated_cell(
                                        ui,
                                        &attr.code,
                                        Font::mono_meta(density),
                                        tokens.text_secondary,
                                        Some(&attr.code),
                                    );
                                });
                                row.col(|ui| {
                                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                        ui.add_space(space::S1);
                                        let val_display = if attr.value.is_empty() {
                                            "—"
                                        } else {
                                            attr.value.as_str()
                                        };
                                        let resp = ui
                                            .add(
                                                egui::Label::new(
                                                    RichText::new(val_display)
                                                        .font(Font::mono_meta(density))
                                                        .color(if attr.value.is_empty() {
                                                            tokens.text_muted
                                                        } else {
                                                            tokens.text_primary
                                                        }),
                                                )
                                                .sense(egui::Sense::click())
                                                .truncate(),
                                            )
                                            .on_hover_text(if attr.value.is_empty() {
                                                "空值"
                                            } else {
                                                "点击复制属性值"
                                            });
                                        if resp.clicked() && !attr.value.is_empty() {
                                            ui.ctx().copy_text(attr.value.clone());
                                            *copy_notice = Some(format!("已复制 {}", attr.code));
                                        }
                                    });
                                });
                            });
                        });
                });
            });
        });
}

fn preview_instances_tab(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    density: Density,
    layer: &PreviewPayloadLayer,
    state: &mut State,
) {
    let mut object_codes: Vec<String> = layer
        .instances
        .iter()
        .map(|row| row.object_model_code.clone())
        .collect();
    object_codes.sort();
    object_codes.dedup();

    let q = state.preview_filter.trim().to_lowercase();
    let mut filtered: Vec<(usize, &PreviewInstanceRow)> = layer
        .instances
        .iter()
        .enumerate()
        .filter(|(_, row)| instance_matches_filter(row, &q, &state.preview_object_filter))
        .collect();
    sort_filtered_instances(&mut filtered, state.preview_instance_sort);

    let mut selected_idx = state
        .preview_selected_instance
        .filter(|idx| *idx < layer.instances.len());
    if selected_idx.is_none()
        && let Some((first_idx, _)) = filtered.first()
    {
        selected_idx = Some(*first_idx);
        state.preview_selected_instance = selected_idx;
    }

    ui.horizontal_wrapped(|ui| {
        ui.add(
            TextEdit::singleline(&mut state.preview_filter)
                .desired_width(density.px(190.0))
                .hint_text("搜索对象类 / 实例编码 / 参考号 / 属性…"),
        );
        search_clear_button(ui, tokens, density, &mut state.preview_filter);
        let selected_obj = if state.preview_object_filter.is_empty() {
            "全部对象类".to_owned()
        } else {
            state.preview_object_filter.clone()
        };
        ComboBox::from_id_salt("data-publish-preview-obj-filter")
            .selected_text(selected_obj)
            .width(density.px(145.0))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.preview_object_filter, String::new(), "全部对象类");
                for code in &object_codes {
                    ui.selectable_value(&mut state.preview_object_filter, code.clone(), code);
                }
            });
        preview_label(
            ui,
            tokens,
            density,
            &format!("显示 {} / {} 条", filtered.len(), layer.instances.len()),
        );
        copy_notice_pill(ui, tokens, density, &state.preview_copy_notice);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(!filtered.is_empty(), widgets::button(tokens, density, "复制表格"))
                .on_hover_text("复制当前筛选结果为制表符分隔表格（可直接粘贴到 Excel）")
                .clicked()
            {
                ui.ctx().copy_text(format_instances_tsv(&filtered));
                state.preview_copy_notice = Some(format!("已复制 {} 条实例", filtered.len()));
            }
        });
    });

    if layer.instances.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("服务端未提取出可发布实例")
                    .font(Font::meta(density))
                    .color(tokens.text_muted),
            );
        });
        return;
    }

    let (table_h, detail_table_h) =
        split_preview_heights(ui.available_height(), selected_idx.is_some(), density);
    let scroll_h = (table_h - density.px(6.0)).max(density.px(60.0));
    let header_h = density.px(26.0);
    let row_h = density.px(24.0);

    egui::Frame::new()
        .fill(tokens.bg_input)
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::same(space::S1 as i8))
        .show(ui, |ui| {
            if filtered.is_empty() {
                ui.set_height(table_h.min(density.px(140.0)));
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("无匹配的提取实例")
                            .font(Font::meta(density))
                            .color(tokens.text_muted),
                    );
                });
                return;
            }
            ui.allocate_ui(egui::vec2(ui.available_width(), table_h), |ui| {
                ScrollArea::horizontal()
                    .id_salt("data-publish-preview-instances-hscroll")
                    .auto_shrink([false, true])
                    .max_height(table_h)
                    .show(ui, |ui| {
                        ui.push_id("data-publish-preview-instances-table", |ui| {
                            configure_selectable_table_style(ui, tokens);
                            TableBuilder::new(ui)
                                .striped(true)
                                .resizable(true)
                                .vscroll(true)
                                .min_scrolled_height(0.0)
                                .max_scroll_height(scroll_h)
                                .sense(egui::Sense::click())
                            .cell_layout(Layout::left_to_right(Align::Center))
                            .column(Column::exact(density.px(36.0)))
                            .column(Column::initial(density.px(148.0)).at_least(density.px(90.0)))
                            .column(Column::initial(density.px(176.0)).at_least(density.px(110.0)))
                            .column(Column::initial(density.px(112.0)).at_least(density.px(80.0)))
                            .column(Column::initial(density.px(60.0)).at_least(density.px(44.0)))
                            .column(Column::initial(density.px(54.0)).at_least(density.px(42.0)))
                            .column(Column::initial(density.px(60.0)).at_least(density.px(48.0)))
                            .column(Column::remainder().at_least(density.px(220.0)))
                            .header(header_h, |mut header| {
                                let current_sort = state.preview_instance_sort;
                                let active_dir = move |col: InstanceSortCol| {
                                    current_sort.and_then(|(c, d)| (c == col).then_some(d))
                                };
                                header.col(|ui| static_header_cell(ui, tokens, density, "#"));
                                header.col(|ui| {
                                    if sortable_header_cell(
                                        ui,
                                        tokens,
                                        density,
                                        "对象类编码",
                                        active_dir(InstanceSortCol::ObjectCode),
                                    ) {
                                        toggle_sort(
                                            &mut state.preview_instance_sort,
                                            InstanceSortCol::ObjectCode,
                                        );
                                    }
                                });
                                header.col(|ui| {
                                    if sortable_header_cell(
                                        ui,
                                        tokens,
                                        density,
                                        "实例编码",
                                        active_dir(InstanceSortCol::InstanceCode),
                                    ) {
                                        toggle_sort(
                                            &mut state.preview_instance_sort,
                                            InstanceSortCol::InstanceCode,
                                        );
                                    }
                                });
                                header.col(|ui| {
                                    if sortable_header_cell(
                                        ui,
                                        tokens,
                                        density,
                                        "来源参考号",
                                        active_dir(InstanceSortCol::Refno),
                                    ) {
                                        toggle_sort(
                                            &mut state.preview_instance_sort,
                                            InstanceSortCol::Refno,
                                        );
                                    }
                                });
                                header.col(|ui| {
                                    if sortable_header_cell(
                                        ui,
                                        tokens,
                                        density,
                                        "操作",
                                        active_dir(InstanceSortCol::Operate),
                                    ) {
                                        toggle_sort(
                                            &mut state.preview_instance_sort,
                                            InstanceSortCol::Operate,
                                        );
                                    }
                                });
                                header.col(|ui| {
                                    if sortable_header_cell(
                                        ui,
                                        tokens,
                                        density,
                                        "版本",
                                        active_dir(InstanceSortCol::Version),
                                    ) {
                                        toggle_sort(
                                            &mut state.preview_instance_sort,
                                            InstanceSortCol::Version,
                                        );
                                    }
                                });
                                header.col(|ui| {
                                    if sortable_header_cell(
                                        ui,
                                        tokens,
                                        density,
                                        "属性数",
                                        active_dir(InstanceSortCol::AttrCount),
                                    ) {
                                        toggle_sort(
                                            &mut state.preview_instance_sort,
                                            InstanceSortCol::AttrCount,
                                        );
                                    }
                                });
                                header.col(|ui| {
                                    static_header_cell(ui, tokens, density, "关键属性摘要")
                                });
                            })
                            .body(|body| {
                                body.rows(row_h, filtered.len(), |mut row| {
                                    let display_idx = row.index();
                                    let (orig_idx, item) = filtered[display_idx];
                                    let is_selected = selected_idx == Some(orig_idx);
                                    row.set_selected(is_selected);
                                    let (fg_primary, fg_secondary, fg_muted) =
                                        row_text_colors(tokens, is_selected);

                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &format!("{}", display_idx + 1),
                                            Font::mono_meta(density),
                                            fg_muted,
                                            None,
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &item.object_model_code,
                                            Font::mono_meta(density),
                                            fg_primary,
                                            Some(&item.object_model_code),
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &item.instance_code,
                                            Font::body(density),
                                            fg_primary,
                                            Some(&item.instance_code),
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            item.refno.as_deref().unwrap_or(""),
                                            Font::mono_meta(density),
                                            fg_secondary,
                                            item.refno.as_deref(),
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &item.operate,
                                            Font::meta(density),
                                            fg_secondary,
                                            None,
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &item.version,
                                            Font::meta(density),
                                            fg_secondary,
                                            None,
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &item.attributes.len().to_string(),
                                            Font::mono_meta(density),
                                            fg_secondary,
                                            None,
                                        );
                                    });
                                    row.col(|ui| {
                                        let summary = summarize_attributes(&item.attributes, 3);
                                        let full_tip = format_attributes_tooltip(&item.attributes);
                                        truncated_cell(
                                            ui,
                                            &summary,
                                            Font::micro(density),
                                            fg_muted,
                                            Some(&full_tip),
                                        );
                                    });
                                    let row_resp = row
                                        .response()
                                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                                    if row_resp.clicked() {
                                        state.preview_selected_instance = Some(orig_idx);
                                    }
                                });
                                });
                        });
                });
            });
        });

    if let Some(idx) = state.preview_selected_instance
        && let Some(instance) = layer.instances.get(idx)
    {
        let label = if instance.instance_code.is_empty() {
            instance.object_model_code.clone()
        } else {
            format!("{} / {}", instance.object_model_code, instance.instance_code)
        };
        render_attribute_detail_panel(
            ui,
            tokens,
            density,
            &label,
            &instance.attributes,
            &mut state.preview_attr_filter,
            &mut state.preview_copy_notice,
            "data-publish-preview-instance-attrs-table",
            detail_table_h,
        );
    }
}

fn preview_relations_tab(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    density: Density,
    layer: &PreviewPayloadLayer,
    state: &mut State,
) {
    let mut rel_types: Vec<String> = layer
        .relations
        .iter()
        .map(|row| row.object_model_code.clone())
        .collect();
    rel_types.sort();
    rel_types.dedup();

    let q = state.preview_relation_filter.trim().to_lowercase();
    let mut filtered: Vec<(usize, &PreviewRelationRow)> = layer
        .relations
        .iter()
        .enumerate()
        .filter(|(_, row)| relation_matches_filter(row, &q, &state.preview_relation_type_filter))
        .collect();
    sort_filtered_relations(&mut filtered, state.preview_relation_sort);

    let mut selected_idx = state
        .preview_selected_relation
        .filter(|idx| *idx < layer.relations.len());
    if selected_idx.is_none()
        && let Some((first_idx, _)) = filtered.first()
    {
        selected_idx = Some(*first_idx);
        state.preview_selected_relation = selected_idx;
    }

    ui.horizontal_wrapped(|ui| {
        ui.add(
            TextEdit::singleline(&mut state.preview_relation_filter)
                .desired_width(density.px(190.0))
                .hint_text("搜索关系类 / 起点 / 终点 / 属性…"),
        );
        search_clear_button(ui, tokens, density, &mut state.preview_relation_filter);
        let selected_type = if state.preview_relation_type_filter.is_empty() {
            "全部关系类".to_owned()
        } else {
            state.preview_relation_type_filter.clone()
        };
        ComboBox::from_id_salt("data-publish-preview-rel-filter")
            .selected_text(selected_type)
            .width(density.px(145.0))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut state.preview_relation_type_filter,
                    String::new(),
                    "全部关系类",
                );
                for code in &rel_types {
                    ui.selectable_value(
                        &mut state.preview_relation_type_filter,
                        code.clone(),
                        code,
                    );
                }
            });
        preview_label(
            ui,
            tokens,
            density,
            &format!("显示 {} / {} 条", filtered.len(), layer.relations.len()),
        );
        copy_notice_pill(ui, tokens, density, &state.preview_copy_notice);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(!filtered.is_empty(), widgets::button(tokens, density, "复制表格"))
                .on_hover_text("复制当前筛选结果为制表符分隔表格（可直接粘贴到 Excel）")
                .clicked()
            {
                ui.ctx().copy_text(format_relations_tsv(&filtered));
                state.preview_copy_notice = Some(format!("已复制 {} 条关系", filtered.len()));
            }
        });
    });

    if layer.relations.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("本次预览无关联关系")
                    .font(Font::meta(density))
                    .color(tokens.text_muted),
            );
        });
        return;
    }

    let (table_h, detail_table_h) =
        split_preview_heights(ui.available_height(), selected_idx.is_some(), density);
    let scroll_h = (table_h - density.px(6.0)).max(density.px(60.0));
    let header_h = density.px(26.0);
    let row_h = density.px(24.0);

    egui::Frame::new()
        .fill(tokens.bg_input)
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::same(space::S1 as i8))
        .show(ui, |ui| {
            if filtered.is_empty() {
                ui.set_height(table_h.min(density.px(140.0)));
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("无匹配的关联关系")
                            .font(Font::meta(density))
                            .color(tokens.text_muted),
                    );
                });
                return;
            }
            ui.allocate_ui(egui::vec2(ui.available_width(), table_h), |ui| {
                ScrollArea::horizontal()
                    .id_salt("data-publish-preview-relations-hscroll")
                    .auto_shrink([false, true])
                    .max_height(table_h)
                    .show(ui, |ui| {
                        ui.push_id("data-publish-preview-relations-table", |ui| {
                            configure_selectable_table_style(ui, tokens);
                            TableBuilder::new(ui)
                                .striped(true)
                                .resizable(true)
                                .vscroll(true)
                                .min_scrolled_height(0.0)
                                .max_scroll_height(scroll_h)
                                .sense(egui::Sense::click())
                            .cell_layout(Layout::left_to_right(Align::Center))
                            .column(Column::exact(density.px(36.0)))
                            .column(Column::initial(density.px(136.0)).at_least(density.px(90.0)))
                            .column(Column::initial(density.px(56.0)).at_least(density.px(42.0)))
                            .column(Column::initial(density.px(54.0)).at_least(density.px(42.0)))
                            .column(Column::initial(density.px(206.0)).at_least(density.px(120.0)))
                            .column(Column::initial(density.px(206.0)).at_least(density.px(120.0)))
                            .column(Column::initial(density.px(58.0)).at_least(density.px(46.0)))
                            .column(Column::remainder().at_least(density.px(200.0)))
                            .header(header_h, |mut header| {
                                let current_sort = state.preview_relation_sort;
                                let active_dir = move |col: RelationSortCol| {
                                    current_sort.and_then(|(c, d)| (c == col).then_some(d))
                                };
                                header.col(|ui| static_header_cell(ui, tokens, density, "#"));
                                header.col(|ui| {
                                    if sortable_header_cell(
                                        ui,
                                        tokens,
                                        density,
                                        "关系类编码",
                                        active_dir(RelationSortCol::ObjectCode),
                                    ) {
                                        toggle_sort(
                                            &mut state.preview_relation_sort,
                                            RelationSortCol::ObjectCode,
                                        );
                                    }
                                });
                                header.col(|ui| {
                                    if sortable_header_cell(
                                        ui,
                                        tokens,
                                        density,
                                        "操作",
                                        active_dir(RelationSortCol::Operate),
                                    ) {
                                        toggle_sort(
                                            &mut state.preview_relation_sort,
                                            RelationSortCol::Operate,
                                        );
                                    }
                                });
                                header.col(|ui| {
                                    if sortable_header_cell(
                                        ui,
                                        tokens,
                                        density,
                                        "版本",
                                        active_dir(RelationSortCol::Version),
                                    ) {
                                        toggle_sort(
                                            &mut state.preview_relation_sort,
                                            RelationSortCol::Version,
                                        );
                                    }
                                });
                                header.col(|ui| {
                                    if sortable_header_cell(
                                        ui,
                                        tokens,
                                        density,
                                        "起点 / 主对象",
                                        active_dir(RelationSortCol::FromSummary),
                                    ) {
                                        toggle_sort(
                                            &mut state.preview_relation_sort,
                                            RelationSortCol::FromSummary,
                                        );
                                    }
                                });
                                header.col(|ui| {
                                    if sortable_header_cell(
                                        ui,
                                        tokens,
                                        density,
                                        "终点 / 关联对象",
                                        active_dir(RelationSortCol::ToSummary),
                                    ) {
                                        toggle_sort(
                                            &mut state.preview_relation_sort,
                                            RelationSortCol::ToSummary,
                                        );
                                    }
                                });
                                header.col(|ui| {
                                    if sortable_header_cell(
                                        ui,
                                        tokens,
                                        density,
                                        "属性数",
                                        active_dir(RelationSortCol::AttrCount),
                                    ) {
                                        toggle_sort(
                                            &mut state.preview_relation_sort,
                                            RelationSortCol::AttrCount,
                                        );
                                    }
                                });
                                header.col(|ui| {
                                    static_header_cell(ui, tokens, density, "属性摘要")
                                });
                            })
                            .body(|body| {
                                body.rows(row_h, filtered.len(), |mut row| {
                                    let display_idx = row.index();
                                    let (orig_idx, rel) = filtered[display_idx];
                                    let is_selected = selected_idx == Some(orig_idx);
                                    row.set_selected(is_selected);
                                    let (fg_primary, fg_secondary, fg_muted) =
                                        row_text_colors(tokens, is_selected);

                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &format!("{}", display_idx + 1),
                                            Font::mono_meta(density),
                                            fg_muted,
                                            None,
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &rel.object_model_code,
                                            Font::mono_meta(density),
                                            fg_primary,
                                            Some(&rel.object_model_code),
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &rel.operate,
                                            Font::meta(density),
                                            fg_secondary,
                                            None,
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &rel.version,
                                            Font::meta(density),
                                            fg_secondary,
                                            None,
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &rel.from_summary,
                                            Font::body(density),
                                            fg_secondary,
                                            Some(&rel.from_summary),
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &rel.to_summary,
                                            Font::body(density),
                                            fg_secondary,
                                            Some(&rel.to_summary),
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            &rel.attributes.len().to_string(),
                                            Font::mono_meta(density),
                                            fg_secondary,
                                            None,
                                        );
                                    });
                                    row.col(|ui| {
                                        let summary = summarize_attributes(&rel.attributes, 3);
                                        let full_tip = format_attributes_tooltip(&rel.attributes);
                                        truncated_cell(
                                            ui,
                                            &summary,
                                            Font::micro(density),
                                            fg_muted,
                                            Some(&full_tip),
                                        );
                                    });
                                    let row_resp = row
                                        .response()
                                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                                    if row_resp.clicked() {
                                        state.preview_selected_relation = Some(orig_idx);
                                    }
                                });
                                });
                        });
                });
            });
        });

    if let Some(idx) = state.preview_selected_relation
        && let Some(rel) = layer.relations.get(idx)
    {
        let label = format!("{} ({} -> {})", rel.object_model_code, rel.from_summary, rel.to_summary);
        render_attribute_detail_panel(
            ui,
            tokens,
            density,
            &label,
            &rel.attributes,
            &mut state.preview_attr_filter,
            &mut state.preview_copy_notice,
            "data-publish-preview-relation-attrs-table",
            detail_table_h,
        );
    }
}

fn preview_skipped_tab(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    density: Density,
    skipped: &[PreviewSkippedRow],
    state: &mut State,
) {
    let q = state.preview_skipped_filter.trim().to_lowercase();
    let filtered: Vec<(usize, &PreviewSkippedRow)> = skipped
        .iter()
        .enumerate()
        .filter(|(_, item)| skipped_matches_filter(item, &q))
        .collect();

    ui.horizontal_wrapped(|ui| {
        ui.add(
            TextEdit::singleline(&mut state.preview_skipped_filter)
                .desired_width(density.px(200.0))
                .hint_text("搜索参考号或跳过原因…"),
        );
        search_clear_button(ui, tokens, density, &mut state.preview_skipped_filter);
        preview_label(
            ui,
            tokens,
            density,
            &format!("显示 {} / {} 条", filtered.len(), skipped.len()),
        );
        copy_notice_pill(ui, tokens, density, &state.preview_copy_notice);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(!filtered.is_empty(), widgets::button(tokens, density, "复制表格"))
                .clicked()
            {
                ui.ctx().copy_text(format_skipped_tsv(&filtered));
                state.preview_copy_notice = Some(format!("已复制 {} 条跳过记录", filtered.len()));
            }
        });
    });

    if skipped.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("无跳过节点")
                    .font(Font::meta(density))
                    .color(tokens.text_muted),
            );
        });
        return;
    }

    let (table_h, _) = split_preview_heights(ui.available_height(), false, density);
    let scroll_h = (table_h - density.px(6.0)).max(density.px(60.0));
    let header_h = density.px(26.0);
    let row_h = density.px(24.0);

    egui::Frame::new()
        .fill(tokens.bg_input)
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::same(space::S1 as i8))
        .show(ui, |ui| {
            if filtered.is_empty() {
                ui.set_height(table_h.min(density.px(140.0)));
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("无匹配的跳过节点")
                            .font(Font::meta(density))
                            .color(tokens.text_muted),
                    );
                });
                return;
            }
            ui.allocate_ui(egui::vec2(ui.available_width(), table_h), |ui| {
                ui.push_id("data-publish-preview-skipped-table", |ui| {
                    TableBuilder::new(ui)
                        .striped(true)
                        .resizable(true)
                        .vscroll(true)
                        .min_scrolled_height(0.0)
                        .max_scroll_height(scroll_h)
                        .cell_layout(Layout::left_to_right(Align::Center))
                        .column(Column::exact(density.px(42.0)))
                        .column(Column::initial(density.px(160.0)).at_least(density.px(100.0)))
                        .column(Column::remainder().at_least(density.px(200.0)))
                        .header(header_h, |mut header| {
                            header.col(|ui| static_header_cell(ui, tokens, density, "#"));
                            header.col(|ui| static_header_cell(ui, tokens, density, "参考号（点击复制）"));
                            header.col(|ui| static_header_cell(ui, tokens, density, "跳过原因"));
                        })
                        .body(|body| {
                            body.rows(row_h, filtered.len(), |mut row| {
                                let idx = row.index();
                                let (_, item) = filtered[idx];
                                row.col(|ui| {
                                    truncated_cell(
                                        ui,
                                        &format!("{}", idx + 1),
                                        Font::mono_meta(density),
                                        tokens.text_muted,
                                        None,
                                    );
                                });
                                row.col(|ui| {
                                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                        ui.add_space(space::S1);
                                        if ui
                                            .add(
                                                egui::Label::new(
                                                    RichText::new(&item.refno)
                                                        .font(Font::mono_meta(density))
                                                        .color(tokens.text_secondary),
                                                )
                                                .sense(egui::Sense::click())
                                                .truncate(),
                                            )
                                            .on_hover_text("点击复制参考号")
                                            .clicked()
                                        {
                                            ui.ctx().copy_text(item.refno.clone());
                                            state.preview_copy_notice = Some(format!("已复制 {}", item.refno));
                                        }
                                    });
                                });
                                row.col(|ui| {
                                    truncated_cell(
                                        ui,
                                        &item.reason,
                                        Font::body(density),
                                        tokens.text_primary,
                                        Some(&item.reason),
                                    );
                                });
                            });
                        });
                });
            });
        });
}

fn preview_input_elements_tab(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    density: Density,
    request: &PublishRequest,
    copy_notice: &mut Option<String>,
) {
    ui.horizontal_wrapped(|ui| {
        preview_label(
            ui,
            tokens,
            density,
            &format!("共 {} 个待提资元素", request.elements.len()),
        );
        copy_notice_pill(ui, tokens, density, copy_notice);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(
                    !request.elements.is_empty(),
                    widgets::button(tokens, density, "复制表格"),
                )
                .clicked()
            {
                ui.ctx().copy_text(format_elements_tsv(&request.elements));
                *copy_notice = Some(format!("已复制 {} 个元素", request.elements.len()));
            }
        });
    });

    if request.elements.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("尚未添加提资元素")
                    .font(Font::meta(density))
                    .color(tokens.text_muted),
            );
        });
        return;
    }

    let (table_h, _) = split_preview_heights(ui.available_height(), false, density);
    let scroll_h = (table_h - density.px(6.0)).max(density.px(60.0));
    let header_h = density.px(26.0);
    let row_h = density.px(24.0);

    egui::Frame::new()
        .fill(tokens.bg_input)
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::same(space::S1 as i8))
        .show(ui, |ui| {
            ui.allocate_ui(egui::vec2(ui.available_width(), table_h), |ui| {
                ui.push_id("data-publish-preview-elements-table", |ui| {
                    TableBuilder::new(ui)
                        .striped(true)
                        .resizable(true)
                        .vscroll(true)
                        .min_scrolled_height(0.0)
                        .max_scroll_height(scroll_h)
                        .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::exact(density.px(42.0)))
                    .column(Column::remainder().at_least(density.px(220.0)))
                    .column(Column::initial(density.px(160.0)).at_least(density.px(110.0)))
                    .header(header_h, |mut header| {
                        header.col(|ui| static_header_cell(ui, tokens, density, "#"));
                        header.col(|ui| static_header_cell(ui, tokens, density, "元素名称"));
                        header.col(|ui| static_header_cell(ui, tokens, density, "参考号（点击复制）"));
                    })
                    .body(|body| {
                        body.rows(row_h, request.elements.len(), |mut row| {
                            let idx = row.index();
                            let element = &request.elements[idx];
                            let refno_str = element.refno.to_string();
                            row.col(|ui| {
                                truncated_cell(
                                    ui,
                                    &format!("{}", idx + 1),
                                    Font::mono_meta(density),
                                    tokens.text_muted,
                                    None,
                                );
                            });
                            row.col(|ui| {
                                truncated_cell(
                                    ui,
                                    &element.name,
                                    Font::body(density),
                                    tokens.text_secondary,
                                    Some(&element.name),
                                );
                            });
                            row.col(|ui| {
                                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                    ui.add_space(space::S1);
                                    if ui
                                        .add(
                                            egui::Label::new(
                                                RichText::new(&refno_str)
                                                    .font(Font::mono_meta(density))
                                                    .color(tokens.text_secondary),
                                            )
                                            .sense(egui::Sense::click())
                                            .truncate(),
                                        )
                                        .on_hover_text("点击复制参考号")
                                        .clicked()
                                    {
                                        ui.ctx().copy_text(refno_str.clone());
                                        *copy_notice = Some(format!("已复制 {refno_str}"));
                                    }
                                });
                                });
                            });
                        });
                });
            });
        });
}

fn preview_label(ui: &mut egui::Ui, tokens: &Tokens, density: Density, text: &str) {
    ui.label(
        RichText::new(text)
            .font(Font::meta(density))
            .color(tokens.text_muted),
    );
}

fn preview_value(ui: &mut egui::Ui, tokens: &Tokens, density: Density, text: &str) {
    ui.label(
        RichText::new(text)
            .font(Font::body(density))
            .color(tokens.text_secondary),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_submits_the_original_request_snapshot() {
        let mut state = State::default();
        state.request.title = "原始标题".into();
        state.request.elements.push(PublishElement {
            refno: 1_u64.into(),
            name: "/PIPE-1".into(),
        });
        assert!(matches!(
            state.start_lookup(),
            Cmd::LookupDataPublishDeletions(_)
        ));
        state.deletion_candidates = vec![
            DeletionCandidate {
                id: "24383/66458".into(),
                name: "/1WCC1135".into(),
                selected: true,
            },
            DeletionCandidate {
                id: "24383/66457".into(),
                name: "/1WCC1134".into(),
                selected: false,
            },
        ];
        state.request.title = "之后修改的标题".into();
        state.submitting = false;

        let Some(Cmd::SubmitDataPublish(request)) = state.confirm_publish() else {
            panic!("确认后应提交原始提资请求");
        };
        assert_eq!(request.title, "原始标题");
        assert_eq!(request.delete_refnos, vec!["24383/66458"]);
        assert!(state.submitting);
        assert!(state.pending_request.is_some());
        assert!(state.confirm_publish().is_none());
    }

    #[test]
    fn state_keeps_unique_elements_and_stable_request_snapshots() {
        let first = PublishElement {
            refno: 1_u64.into(),
            name: "PIPE /P-1001".into(),
        };
        let second = PublishElement {
            refno: 2_u64.into(),
            name: "EQUI /V-1001".into(),
        };
        let mut state = State::default();
        assert_eq!(DesignPhase::default(), DesignPhase::Detailed);
        assert_eq!(state.request.design_phase, DesignPhase::Detailed);
        state.request.title = "全量发布".into();
        state.request.category = PublishCategory::Electrical;
        state.request.design_phase = DesignPhase::Detailed;

        assert!(state.add_element(first.clone()));
        assert!(!state.add_element(first.clone()));
        assert!(state.add_element(second.clone()));
        assert_eq!(state.request.elements, vec![first.clone(), second.clone()]);

        let submitted = state.request.clone();
        state.selected_refno = Some(first.refno);
        assert!(state.remove_selected());
        assert_eq!(state.request.elements, vec![second]);

        state.clear_elements();
        assert!(state.request.elements.is_empty());
        assert_eq!(submitted.title, "全量发布");
        assert_eq!(submitted.category, PublishCategory::Electrical);
        assert_eq!(submitted.category.endpoint(), "/get_dq_bran_data");
        assert_eq!(submitted.design_phase, DesignPhase::Detailed);
        assert_eq!(PublishCategory::ALL.len(), 5);
        assert_eq!(PublishCategory::Room.label(), "设备房间位置信息发布");
        assert_eq!(PublishCategory::Room.endpoint(), "/get_bran_data_room_code");
        assert_eq!(
            submitted.elements,
            vec![
                first,
                PublishElement {
                    refno: 2_u64.into(),
                    name: "EQUI /V-1001".into(),
                }
            ]
        );
    }

    #[test]
    fn start_preview_maps_supported_and_unsupported_categories() {
        assert_eq!(PublishCategory::Process.preview_route(), Some(("pipe", "bran")));
        assert_eq!(PublishCategory::Electrical.preview_route(), Some(("elec", "bran")));
        assert_eq!(PublishCategory::Instrumentation.preview_route(), Some(("inst", "bran")));
        assert_eq!(PublishCategory::Ventilation.preview_route(), Some(("hvac", "bran")));
        assert_eq!(PublishCategory::Room.preview_route(), None);

        let mut state = State::default();
        assert!(state.start_preview().is_none());
        assert!(state.preview_open);
        assert!(matches!(state.preview_status, PreviewStatus::Unsupported(_)));
        assert_eq!(state.preview_tab, PreviewTab::InputElements);

        state.add_element(PublishElement {
            refno: RefU64::from_two_nums(24383, 68341),
            name: "/1WCC0302A/B1".into(),
        });
        state.request.category = PublishCategory::Instrumentation;
        let cmd = state.start_preview();
        assert!(matches!(cmd, Some(Cmd::PreviewDataPublish(_))));
        assert!(matches!(state.preview_status, PreviewStatus::Loading));
        assert_eq!(state.preview_tab, PreviewTab::Instances);

        state.request.category = PublishCategory::Room;
        assert!(state.start_preview().is_none());
        assert!(matches!(state.preview_status, PreviewStatus::Unsupported(_)));
        assert_eq!(state.preview_tab, PreviewTab::InputElements);
    }

    #[test]
    fn preview_tables_support_filter_sort_and_tsv_export() {
        let inst_a = PreviewInstanceRow {
            object_model_code: "PE_SEG_PIPSEG".into(),
            instance_code: "1WCC1135/B1".into(),
            refno: Some("24383/66459".into()),
            operate: "draft".into(),
            version: "A版".into(),
            attributes: vec![PreviewAttributeRow {
                code: "PBS_ID".into(),
                value: "1WCC1135/B1".into(),
            }],
        };
        let inst_b = PreviewInstanceRow {
            object_model_code: "PE_FIT_PIPCAP".into(),
            instance_code: "24383/66460".into(),
            refno: Some("24383/66460".into()),
            operate: "draft".into(),
            version: "A版".into(),
            attributes: vec![
                PreviewAttributeRow {
                    code: "PBS_ID".into(),
                    value: "24383/66460".into(),
                },
                PreviewAttributeRow {
                    code: "TYP_COE".into(),
                    value: "CAP".into(),
                },
            ],
        };
        let mut inst_rows = vec![(0, &inst_a), (1, &inst_b)];
        sort_filtered_instances(&mut inst_rows, Some((InstanceSortCol::AttrCount, SortDir::Desc)));
        assert_eq!(inst_rows[0].0, 1);
        let inst_tsv = format_instances_tsv(&inst_rows);
        assert!(inst_tsv.contains("#\t对象类编码\t实例编码"));
        assert!(inst_tsv.contains("PE_FIT_PIPCAP\t24383/66460"));

        let rel_1 = PreviewRelationRow {
            object_model_code: "REL_PBS_OBJ".into(),
            operate: "draft".into(),
            version: "A版".into(),
            from_summary: "PE_FIT_PIPCAP: 24383/66460".into(),
            to_summary: "PE_SEG_PIPSEG: 1WCC1135/B1".into(),
            attributes: vec![PreviewAttributeRow {
                code: "START_OBJ_CODE".into(),
                value: "PE_FIT_PIPCAP".into(),
            }],
        };
        let rel_2 = PreviewRelationRow {
            object_model_code: "REL_PBS_3D_MOD".into(),
            operate: "draft".into(),
            version: "A版".into(),
            from_summary: "1WCC1135/B1".into(),
            to_summary: "1WCC-PIPE-RX".into(),
            attributes: vec![
                PreviewAttributeRow {
                    code: "PBS_ID".into(),
                    value: "1WCC1135/B1".into(),
                },
                PreviewAttributeRow {
                    code: "THR_DIM_MOD_ID".into(),
                    value: "1WCC-PIPE-RX".into(),
                },
            ],
        };
        assert!(relation_matches_filter(&rel_1, "24383_66460", ""));
        assert!(relation_matches_filter(&rel_2, "", "REL_PBS_3D_MOD"));
        assert!(!relation_matches_filter(&rel_1, "", "REL_PBS_3D_MOD"));

        let mut rel_rows = vec![(0, &rel_1), (1, &rel_2)];
        sort_filtered_relations(&mut rel_rows, Some((RelationSortCol::ObjectCode, SortDir::Asc)));
        assert_eq!(rel_rows[0].1.object_model_code, "REL_PBS_3D_MOD");

        let rel_tsv = format_relations_tsv(&rel_rows);
        assert!(rel_tsv.contains("REL_PBS_3D_MOD"));
        assert!(rel_tsv.contains("THR_DIM_MOD_ID=1WCC-PIPE-RX"));

        let attr_tsv = format_attributes_tsv(&rel_2.attributes);
        assert_eq!(
            attr_tsv,
            "属性编码\t属性值\nPBS_ID\t1WCC1135/B1\nTHR_DIM_MOD_ID\t1WCC-PIPE-RX"
        );
    }

    #[test]
    fn split_preview_heights_stays_bounded_across_window_sizes() {
        let d = Density::Standard;
        for avail in [260.0_f32, 360.0, 500.0, 720.0] {
            let (main_no_detail, detail_zero) = split_preview_heights(avail, false, d);
            assert!(main_no_detail <= avail);
            assert_eq!(detail_zero, 0.0);

            let (main_h, detail_h) = split_preview_heights(avail, true, d);
            assert!(main_h >= 96.0);
            assert!(detail_h >= 72.0);
            assert!(main_h + detail_h < avail, "avail={avail}: {main_h} + {detail_h}");
        }
    }

    #[test]
    fn row_text_colors_switches_to_accent_ink_when_selected() {
        for tokens in [Tokens::light(), Tokens::dark()] {
            let (p_unsel, s_unsel, m_unsel) = row_text_colors(&tokens, false);
            assert_eq!(p_unsel, tokens.text_primary);
            assert_eq!(s_unsel, tokens.text_secondary);
            assert_eq!(m_unsel, tokens.text_muted);

            let (p_sel, s_sel, m_sel) = row_text_colors(&tokens, true);
            assert_eq!(p_sel, tokens.accent_ink);
            assert_ne!(s_sel, tokens.text_secondary);
            assert_ne!(m_sel, tokens.text_muted);
        }
    }

    #[test]
    fn truncated_cell_does_not_steal_table_row_clicks() {
        let ctx = egui::Context::default();
        let tokens = Tokens::light();
        let d = Density::Standard;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 240.0));
        let run_frame = |events: Vec<egui::Event>| -> (Option<egui::Pos2>, bool) {
            let mut cell_center = None;
            let mut clicked = false;
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            });
            egui::Area::new("test-table-row-click".into())
                .fixed_pos(screen.min)
                .show(&ctx, |ui| {
                    ui.set_min_size(screen.size());
                    ui.set_max_size(screen.size());
                    ui.push_id("test-table-row-click", |ui| {
                        configure_selectable_table_style(ui, &tokens);
                        TableBuilder::new(ui)
                            .striped(true)
                            .sense(egui::Sense::click())
                            .cell_layout(Layout::left_to_right(Align::Center))
                            .column(Column::exact(180.0))
                            .column(Column::exact(180.0))
                            .body(|body| {
                                body.rows(24.0, 1, |mut row| {
                                    let (rect, _) = row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            "PE_FIT_ELB",
                                            Font::mono_meta(d),
                                            tokens.text_primary,
                                            Some("PE_FIT_ELB"),
                                        );
                                    });
                                    row.col(|ui| {
                                        truncated_cell(
                                            ui,
                                            "24383/67121",
                                            Font::body(d),
                                            tokens.text_secondary,
                                            None,
                                        );
                                    });
                                    cell_center = Some(rect.center());
                                    if row.response().clicked() {
                                        clicked = true;
                                    }
                                });
                            });
                    });
                });
            let _ = ctx.end_pass();
            (cell_center, clicked)
        };

        let _ = run_frame(Vec::new());
        let (cell_center, _) = run_frame(Vec::new());
        let pos = cell_center.expect("should record cell center");
        let (_, c1) = run_frame(vec![egui::Event::PointerMoved(pos)]);
        let (_, c2) = run_frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        let (_, c3) = run_frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(
            c1 || c2 || c3,
            "clicking directly on truncated_cell text should trigger row click"
        );
    }
}
