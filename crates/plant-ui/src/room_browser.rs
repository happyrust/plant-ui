//! 房间浏览器浮窗（S14 稿）：全部房间的总览表 + 检索 + 四态筛选。
//!
//! 浮窗而不是 dock 页签（计划决定 3）：打开频率低，常驻会挤占 dock。全表是
//! 全库扫描级的重查询（计划风险 5），数据只在 `Cmd::OpenRoomBrowser` 时拉，
//! 检索与筛选全部本地过滤，不回库。
//!
//! 行操作只有「聚焦」一枚（`Cmd::FocusRoom`：隔离 + 取景 + 房间页签详情）。
//! 设计稿画了定位 / 隔离两枚，但宿主的房间视图本来就是两件事一次做完，
//! 拆成两枚按钮只是让人按两次。空房间（无面板无成员）无处取景，置灰。
//! 命名不合规的房间不参与归属计算，红色警示，也无处取景。

use egui::{
    Align2, CornerRadius, CursorIcon, FontId, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui,
    Vec2, vec2,
};
use egui_phosphor::regular as ph;

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens, radius, space};
use crate::style::widgets;
use crate::{Cmd, RefU64};

/// 浏览器数据状态。`Idle` = 从没拉过表（窗口没开过）；四态语义同各面板 Vm。
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Vm {
    #[default]
    Idle,
    /// 查询在途。刷新时保留上一份撑住表格，状态栏标「刷新中」。
    Loading(Option<Vec<Row>>),
    Ready(Vec<Row>),
    Failed(String),
}

impl Vm {
    /// 开始（重新）拉全表，防闪烁语义同 `PropsVm::begin_query`。
    pub fn begin_query(&mut self) {
        let previous = match std::mem::take(self) {
            Self::Ready(rows) | Self::Loading(Some(rows)) => Some(rows),
            _ => None,
        };
        *self = Self::Loading(previous);
    }

    pub fn loading(&self) -> bool {
        matches!(self, Self::Loading(_))
    }
}

/// 一行房间（数据层 `room::RoomOverviewRow` 的绘制层镜像）。
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub room: RefU64,
    /// 房间 FRMW 全名（如 `/1RX-RM03-R301`）。
    pub name: String,
    pub room_num: String,
    /// 如 `1RX-R301`；命名不合规时通常解不出来。
    pub room_code: Option<String>,
    /// 房号是否通过命名校验；不合规的不参与归属计算。
    pub valid_name: bool,
    pub panel_count: usize,
    pub member_count: usize,
    pub pending_recalc: bool,
}

impl Row {
    /// 状态列：不合规 > 待重算 > 空房间 > 已就绪，一行只说最要紧的一件事。
    fn state(&self) -> RowState {
        if !self.valid_name {
            RowState::Invalid
        } else if self.pending_recalc {
            RowState::Pending
        } else if self.member_count == 0 {
            RowState::Empty
        } else {
            RowState::Ok
        }
    }

    /// 有没有东西可取景（面板或成员至少一样）。
    fn focusable(&self) -> bool {
        self.panel_count > 0 || self.member_count > 0
    }

    fn matches(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let hit = |s: &str| s.to_lowercase().contains(needle);
        hit(&self.room_num) || hit(&self.name) || self.room_code.as_deref().is_some_and(hit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowState {
    Ok,
    Pending,
    Empty,
    Invalid,
}

/// 四态筛选（S14 的控制条芯片）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Filter {
    #[default]
    All,
    Pending,
    Empty,
    Invalid,
}

impl Filter {
    fn admits(self, row: &Row) -> bool {
        match self {
            Filter::All => true,
            Filter::Pending => row.pending_recalc,
            Filter::Empty => row.member_count == 0,
            Filter::Invalid => !row.valid_name,
        }
    }
}

/// 窗口自身的状态（开合 / 检索串 / 筛选档），界面状态不进 Vm。
#[derive(Default)]
pub struct State {
    pub open: bool,
    search: String,
    filter: Filter,
}

// ================================================================ 窗口外壳

pub fn show(ctx: &egui::Context, t: &Tokens, d: Density, vm: &Vm, state: &mut State) -> Vec<Cmd> {
    if !state.open {
        return Vec::new();
    }
    let mut cmds = Vec::new();
    let mut open = state.open;
    egui::Window::new("房间浏览器")
        .id(egui::Id::new("room-browser-window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(true)
        .default_size([d.px(1040.0), d.px(720.0)])
        .min_size([d.px(720.0), d.px(420.0)])
        .frame(
            egui::Frame::new()
                .fill(t.bg_panel)
                .corner_radius(CornerRadius::same(radius::LG))
                .stroke(Stroke::new(1.0_f32, t.border_strong)),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            header(ui, t, d, vm, &mut open, &mut cmds);
            match vm {
                Vm::Idle | Vm::Loading(None) => loading_body(ui, t, d),
                Vm::Failed(reason) => failure_body(ui, t, d, reason, &mut cmds),
                Vm::Ready(rows) | Vm::Loading(Some(rows)) => {
                    table(ui, t, d, rows, vm.loading(), state, &mut cmds)
                }
            }
        });
    state.open = open;
    cmds
}

fn header(ui: &mut Ui, t: &Tokens, d: Density, vm: &Vm, open: &mut bool, cmds: &mut Vec<Cmd>) {
    let h = d.px(52.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius {
            nw: radius::LG,
            ne: radius::LG,
            ..CornerRadius::ZERO
        },
        t.bg_chrome,
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(d.px(16.0), 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let ui = &mut child;
    ui.spacing_mut().item_spacing.x = d.px(12.0);

    let (chip, _) = ui.allocate_exact_size(Vec2::splat(d.px(28.0)), Sense::hover());
    ui.painter()
        .rect_filled(chip, CornerRadius::same(radius::MD), t.bg_hover);
    ui.painter().text(
        chip.center(),
        Align2::CENTER_CENTER,
        ph::DOOR_OPEN,
        FontId::proportional(d.px(15.0)),
        t.accent,
    );
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(
            RichText::new("房间浏览器")
                .font(Font::title(d))
                .color(t.text_primary),
        );
        ui.label(
            RichText::new("全部房间与归属状态 · 从右键菜单或「房间」页签进入")
                .font(Font::meta(d))
                .color(t.text_muted),
        );
    });
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.add(widgets::tool_btn(t, d, ph::X, false)).clicked() {
            *open = false;
        }
        let refresh = ui.add_enabled(
            !vm.loading(),
            widgets::tool_btn(t, d, ph::ARROWS_CLOCKWISE, false),
        );
        if refresh
            .on_hover_text("重新统计全部房间（全库扫描级，几十秒）")
            .on_disabled_hover_text("正在统计中…")
            .clicked()
        {
            cmds.push(Cmd::OpenRoomBrowser);
        }
        ui.label(
            RichText::new("room_relate · room_panel_relate")
                .font(Font::mono_meta(d))
                .color(t.text_muted),
        );
    });
    hairline(ui, t);
}

fn loading_body(ui: &mut Ui, t: &Tokens, d: Density) {
    widgets::pane_note(
        ui,
        t,
        d,
        widgets::PaneNote {
            state: widgets::PaneState::Loading,
            icon: ph::SPINNER,
            text: "正在统计全部房间…",
            detail: Some("全库扫描级查询，几十秒是正常的；关闭窗口不会中断它"),
            retry: false,
        },
    );
}

fn failure_body(ui: &mut Ui, t: &Tokens, d: Density, reason: &str, cmds: &mut Vec<Cmd>) {
    if widgets::pane_note(
        ui,
        t,
        d,
        widgets::PaneNote {
            state: widgets::PaneState::Error,
            icon: ph::WARNING,
            text: reason,
            detail: None,
            retry: true,
        },
    ) {
        cmds.push(Cmd::OpenRoomBrowser);
    }
}

// ================================================================ 表格

/// 固定列宽（S14 稿）：房号 150 / 面板 70 / 成员 80 / 状态 120 / 操作 70，
/// 名称列吃掉剩余。列头与数据行共用一套，错开就是两处一起错。
fn layout_cells(total: f32, d: Density) -> [f32; 6] {
    let cols = [150.0, 70.0, 80.0, 120.0, 70.0].map(|w| d.px(w));
    let gaps = d.px(8.0) * 5.0;
    let name = (total - cols.iter().sum::<f32>() - gaps).max(d.px(120.0));
    [cols[0], name, cols[1], cols[2], cols[3], cols[4]]
}

fn table(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    rows: &[Row],
    refreshing: bool,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    if rows.is_empty() {
        widgets::pane_note(
            ui,
            t,
            d,
            widgets::PaneNote {
                state: widgets::PaneState::Empty,
                icon: ph::DOOR_OPEN,
                text: "库里没有识别到房间",
                detail: Some("房间 = 名字含房间关键词（如 -RM）的 FRMW"),
                retry: false,
            },
        );
        return;
    }
    controls(ui, t, d, rows, state);
    column_head(ui, t, d);

    let needle = state.search.trim().to_lowercase();
    let visible: Vec<&Row> = rows
        .iter()
        .filter(|row| state.filter.admits(row) && row.matches(&needle))
        .collect();
    let shown = visible.len();

    let status_h = d.px(32.0);
    let body_h = (ui.available_height() - status_h).max(d.px(80.0));
    let width = ui.available_width();
    ui.allocate_ui(vec2(width, body_h), |ui| {
        ui.set_min_size(vec2(width, body_h));
        if visible.is_empty() {
            let (rect, _) = ui.allocate_exact_size(vec2(width, d.px(80.0)), Sense::hover());
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "没有匹配的房间",
                Font::meta(d),
                t.text_muted,
            );
            return;
        }
        let row_h = d.px(34.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        // 435 间量级实测流畅（计划 M6 验收口径）；行高一致，虚拟滚动只画视野内的行。
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_h, visible.len(), |ui, range| {
                for row in &visible[range] {
                    room_row(ui, t, d, row, cmds);
                }
            });
    });
    status_bar(ui, t, d, rows, shown, refreshing);
}

fn controls(ui: &mut Ui, t: &Tokens, d: Density, rows: &[Row], state: &mut State) {
    let h = d.px(44.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(d.px(16.0), 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let cui = &mut child;
    cui.spacing_mut().item_spacing.x = d.px(8.0);
    cui.add(
        TextEdit::singleline(&mut state.search)
            .id(egui::Id::new("room-browser-search"))
            .desired_width(d.px(240.0))
            .font(Font::meta(d))
            .background_color(t.bg_input)
            .text_color(t.text_primary)
            .hint_text(RichText::new("搜索房号 / 房间码 / 房间名…").color(t.text_muted)),
    );
    let pending = rows.iter().filter(|r| r.pending_recalc).count();
    let empty = rows.iter().filter(|r| r.member_count == 0).count();
    let invalid = rows.iter().filter(|r| !r.valid_name).count();
    let chips = [
        (Filter::All, format!("全部 {}", rows.len())),
        (Filter::Pending, format!("待重算 {pending}")),
        (Filter::Empty, format!("空房间 {empty}")),
        (Filter::Invalid, format!("命名不合规 {invalid}")),
    ];
    for (filter, label) in &chips {
        if cui
            .add(widgets::chip(t, d, label, state.filter == *filter))
            .clicked()
        {
            state.filter = *filter;
        }
    }
    hairline(ui, t);
}

fn column_head(ui: &mut Ui, t: &Tokens, d: Density) {
    let h = d.px(28.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, 0, t.bg_header);
        let pad = d.px(space::S3);
        let cells = layout_cells(rect.width() - pad * 2.0, d);
        let labels = ["房号", "名称", "面板", "成员", "状态", "操作"];
        let mut x = rect.left() + pad;
        for (i, (w, label)) in cells.iter().zip(labels).enumerate() {
            let last = i == labels.len() - 1;
            let (anchor, align) = if last {
                (x + w, Align2::RIGHT_CENTER)
            } else {
                (x, Align2::LEFT_CENTER)
            };
            p.text(
                egui::pos2(anchor, rect.center().y),
                align,
                label,
                Font::micro(d),
                t.text_muted,
            );
            x += w + d.px(8.0);
        }
    }
    hairline(ui, t);
}

fn room_row(ui: &mut Ui, t: &Tokens, d: Density, row: &Row, cmds: &mut Vec<Cmd>) {
    let h = d.px(34.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 0, t.bg_hover);
    }
    p.rect_filled(
        egui::Rect::from_min_max(rect.left_bottom() - vec2(0.0, 1.0), rect.right_bottom()),
        0,
        t.border,
    );
    let pad = d.px(space::S3);
    let cells = layout_cells(rect.width() - pad * 2.0, d);
    let mut x = rect.left() + pad;
    let cy = rect.center().y;

    // 房号：芯片 + 房间码。不合规的芯片换 danger 配色与警示图标。
    {
        let (chip_bg, chip_fg, icon) = if row.valid_name {
            (t.accent_bg, t.accent, ph::DOOR_OPEN)
        } else {
            (t.danger_bg, t.danger, ph::WARNING)
        };
        let label = format!("{icon} {}", row.room_num);
        let g = p.layout_no_wrap(label, Font::mono_micro(d), chip_fg);
        let chip = egui::Rect::from_min_size(
            egui::pos2(x, cy - d.px(9.0)),
            vec2(g.size().x + d.px(12.0), d.px(18.0)),
        );
        p.rect_filled(chip, CornerRadius::same(radius::SM), chip_bg);
        p.galley(
            egui::pos2(chip.left() + d.px(6.0), cy - g.size().y / 2.0),
            g,
            chip_fg,
        );
        if let Some(code) = &row.room_code {
            let code_x = chip.right() + d.px(6.0);
            if code_x < x + cells[0] {
                let clip = p.with_clip_rect(egui::Rect::from_min_max(
                    egui::pos2(code_x, rect.top()),
                    egui::pos2(x + cells[0], rect.bottom()),
                ));
                clip.text(
                    egui::pos2(code_x, cy),
                    Align2::LEFT_CENTER,
                    code,
                    Font::mono_micro(d),
                    t.text_muted,
                );
            }
        }
        x += cells[0] + d.px(8.0);
    }
    // 名称（超宽裁剪，别把后面的列挤走）。
    {
        let clip = p.with_clip_rect(egui::Rect::from_min_max(
            egui::pos2(x, rect.top()),
            egui::pos2(x + cells[1], rect.bottom()),
        ));
        clip.text(
            egui::pos2(x, cy),
            Align2::LEFT_CENTER,
            &row.name,
            Font::mono_meta(d),
            t.text_primary,
        );
        x += cells[1] + d.px(8.0);
    }
    for (value, width) in [(row.panel_count, cells[2]), (row.member_count, cells[3])] {
        p.text(
            egui::pos2(x, cy),
            Align2::LEFT_CENTER,
            value.to_string(),
            Font::mono_meta(d),
            t.text_secondary,
        );
        x += width + d.px(8.0);
    }
    // 状态芯片。
    {
        let (bg, fg, text) = match row.state() {
            RowState::Ok => (t.success_bg, t.success, "已就绪"),
            RowState::Pending => (t.warn_bg, t.warn, "待重算"),
            RowState::Empty => (t.bg_input, t.text_muted, "空房间"),
            RowState::Invalid => (t.danger_bg, t.danger, "命名不合规"),
        };
        let g = p.layout_no_wrap(text.to_owned(), Font::micro(d), fg);
        let chip = egui::Rect::from_min_size(
            egui::pos2(x, cy - d.px(9.0)),
            vec2(g.size().x + d.px(19.0), d.px(18.0)),
        );
        p.rect_filled(chip, CornerRadius::same(radius::SM), bg);
        p.circle_filled(egui::pos2(chip.left() + d.px(8.0), cy), d.px(2.5), fg);
        p.galley(
            egui::pos2(chip.left() + d.px(13.0), cy - g.size().y / 2.0),
            g,
            fg,
        );
        x += cells[4] + d.px(8.0);
    }
    // 操作：聚焦（隔离 + 取景 + 房间页签详情）。
    {
        let btn = egui::Rect::from_center_size(
            egui::pos2(x + cells[5] - d.px(10.0), cy),
            Vec2::splat(d.px(20.0)),
        );
        let focusable = row.focusable();
        let resp = ui.interact(btn, ui.id().with(("room-op", row.room)), Sense::click());
        let fg = if !focusable {
            t.text_muted.linear_multiply(0.5)
        } else if resp.hovered() {
            t.text_primary
        } else {
            t.text_muted
        };
        ui.painter().text(
            btn.center(),
            Align2::CENTER_CENTER,
            ph::CROSSHAIR_SIMPLE,
            FontId::proportional(d.px(13.0)),
            fg,
        );
        let resp = if focusable {
            resp.on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("聚焦：隔离并取景到这间房，详情进「房间」页签")
        } else {
            resp.on_hover_text(if row.valid_name {
                "空房间：没有面板也没有成员，无处取景"
            } else {
                "命名不合规：不参与归属计算，无处取景"
            })
        };
        if focusable && resp.clicked() {
            cmds.push(Cmd::FocusRoom(row.room));
        }
    }
}

fn status_bar(ui: &mut Ui, t: &Tokens, d: Density, rows: &[Row], shown: usize, refreshing: bool) {
    let h = d.px(32.0);
    hairline(ui, t);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h - 1.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    let pending = rows.iter().filter(|r| r.pending_recalc).count();
    let empty = rows.iter().filter(|r| r.member_count == 0).count();
    let invalid = rows.iter().filter(|r| !r.valid_name).count();
    p.text(
        egui::pos2(rect.left() + d.px(16.0), rect.center().y),
        Align2::LEFT_CENTER,
        format!(
            "共 {} 间 · 待重算 {pending} · 空房间 {empty} · 命名不合规 {invalid}",
            rows.len(),
        ),
        Font::mono_micro(d),
        t.text_muted,
    );
    let right = if refreshing {
        "刷新中…".to_owned()
    } else if shown == rows.len() {
        String::new()
    } else {
        format!("筛得 {shown} 间")
    };
    if !right.is_empty() {
        p.text(
            egui::pos2(rect.right() - d.px(16.0), rect.center().y),
            Align2::RIGHT_CENTER,
            right,
            Font::mono_micro(d),
            t.text_muted,
        );
    }
}

/// 1px 分隔线（窗体内部各横栏之间）。
fn hairline(ui: &mut Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0, t.border);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(num: &str, valid: bool, members: usize, pending: bool) -> Row {
        Row {
            room: RefU64(1),
            name: format!("/1RX-RM03-{num}"),
            room_num: num.to_owned(),
            room_code: valid.then(|| format!("1RX-{num}")),
            valid_name: valid,
            panel_count: if valid { 4 } else { 0 },
            member_count: members,
            pending_recalc: pending,
        }
    }

    /// 四档筛选各认各的行，口径钉死：待重算看队列标记、空房间看成员数、
    /// 不合规看命名校验，互不越界。
    #[test]
    fn filters_admit_the_right_rows() {
        let ok = row("R301", true, 128, false);
        let pending = row("R302", true, 86, true);
        let empty = row("R305", true, 0, false);
        let invalid = row("RM12", false, 0, false);
        let all = [&ok, &pending, &empty, &invalid];

        let admitted = |f: Filter| -> Vec<&str> {
            all.iter()
                .filter(|r| f.admits(r))
                .map(|r| r.room_num.as_str())
                .collect()
        };
        assert_eq!(admitted(Filter::All).len(), 4);
        assert_eq!(admitted(Filter::Pending), ["R302"]);
        assert_eq!(admitted(Filter::Empty), ["R305", "RM12"]);
        assert_eq!(admitted(Filter::Invalid), ["RM12"]);
    }

    /// 检索大小写不敏感，房号 / 房间码 / 全名三路都认。
    #[test]
    fn search_matches_num_code_and_name() {
        let r = row("R301", true, 12, false);
        assert!(r.matches("r301"));
        assert!(r.matches("1rx-r301"));
        assert!(r.matches("rm03"));
        assert!(!r.matches("b666"));
        assert!(r.matches(""));
    }

    /// 状态列一行只说最要紧的一件事：不合规 > 待重算 > 空 > 就绪。
    #[test]
    fn row_state_reports_the_most_urgent_fact() {
        assert_eq!(row("R301", true, 12, false).state(), RowState::Ok);
        assert_eq!(row("R301", true, 12, true).state(), RowState::Pending);
        assert_eq!(row("R301", true, 0, false).state(), RowState::Empty);
        // 不合规压过待重算：前者是「这行根本不参与」，后者只是晚点。
        assert_eq!(row("RM12", false, 0, true).state(), RowState::Invalid);
    }
}
