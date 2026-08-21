//! 三维视口：独立壳显示占位纹理，Bevy 宿主显示实时渲染目标。
//!
//! 底是 `viewport-top` -> `viewport-bottom` 的竖直渐变，上面按 S1 那块「模型场景」
//! 矩形等比裁切铺一张占位纹理（0.92 不透明）。等比裁切而不是拉伸：视口的宽高比
//! 跟着 dock 分隔条走，拉伸的话管子会随手一拖就变成椭圆。
//!
//! 视口里的浮层：左上角 HUD、实时渲染时的左侧工具栏、右上角视图立方体
//! （[`super::viewcube`]，S1-B 稿），以及三根世界轴的轴端标签。HUD 末尾带一段
//! 格距读数——网格与三轴是视口里唯一的长度参照，不把一格多长说出来，它们就
//! 只是一组看得见疏密、读不出尺寸的线。光标处的坐标读数仍留白，那是另一类
//! 信息件（要逐帧的射线命中），另轮再说。
//!
//! 鼠标：左键点击拾取、左键 / 中键拖拽平移、右键拖拽旋转、滚轮缩放。

use egui::{
    Align, Align2, Color32, Id, Key, KeyboardShortcut, Layout, Margin, Mesh, Modifiers, Rect,
    RichText, ScrollArea, Sense, Shape, Stroke, Ui, UiBuilder, Vec2, pos2, vec2,
};
use egui_phosphor::regular as ph;

use super::viewcube;
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens, radius};
use crate::style::widgets;
use crate::vm::{RoomViewVm, Selection, View3dVm, WorkbenchVm};
use crate::{CameraGesture, CameraMotion, Cmd, ModelAction, RefU64};

/// 占位纹理的不透明度，取自 S1 的「模型场景」矩形。压这么一点让渐变底透上来，
/// 图与视口连成一片，不像贴上去的一张照片。
const PLACEHOLDER_ALPHA: u8 = 235;

/// 视口工具栏上的一枚按钮。
///
/// 图标、hover 文案、快捷键、命令收在一处：摊到绘制点和快捷键分支里，
/// 改一个键位就得记着改两处，迟早对不上。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Show,
    Hide,
    HideAll,
    ShowAll,
    Focus,
    Fit,
}

impl Tool {
    /// 按显示顺序排。
    ///
    /// 距离测量不在这里：它得宿主给出精确的表面命中才算数，拿世界 AABB 的交点
    /// 冒充就是假测量。等那条链路接通再进这张表，不先摆一枚按不动的尺子。
    const BAR: [Tool; 6] = [
        Tool::Show,
        Tool::Hide,
        Tool::HideAll,
        Tool::ShowAll,
        Tool::Focus,
        Tool::Fit,
    ];

    fn icon(self) -> &'static str {
        match self {
            Tool::Show => ph::EYE,
            Tool::Hide => ph::EYE_SLASH,
            Tool::HideAll => ph::EYE_CLOSED,
            // 复数的眼睛对闭着的眼睛：这一对读作「全部」，与上面单数那一对分开。
            Tool::ShowAll => ph::EYES,
            Tool::Focus => ph::CROSSHAIR_SIMPLE,
            Tool::Fit => ph::CORNERS_OUT,
        }
    }

    /// hover 文案带上快捷键——按钮上只有一个图标，说不出自己是谁。
    fn hint(self) -> &'static str {
        match self {
            Tool::Show => "显示所选 (S)",
            Tool::Hide => "隐藏所选 (H)",
            Tool::HideAll => "隐藏全部 (Shift+H)",
            // 不写「显示所有元素」：它只把已加载的显示回来，不替用户加载没加载过的。
            Tool::ShowAll => "显示全部 (Shift+S)",
            // 不写「定位所选」：多选时它只带相机去主选中那一个，见 `ModelAction::Focus`。
            Tool::Focus => "定位到当前元素 (F)",
            Tool::Fit => "视图适应 (Home)",
        }
    }

    fn shortcut(self) -> KeyboardShortcut {
        match self {
            Tool::Show => KeyboardShortcut::new(Modifiers::NONE, Key::S),
            Tool::Hide => KeyboardShortcut::new(Modifiers::NONE, Key::H),
            Tool::HideAll => KeyboardShortcut::new(Modifiers::SHIFT, Key::H),
            Tool::ShowAll => KeyboardShortcut::new(Modifiers::SHIFT, Key::S),
            Tool::Focus => KeyboardShortcut::new(Modifiers::NONE, Key::F),
            Tool::Fit => KeyboardShortcut::new(Modifiers::NONE, Key::Home),
        }
    }

    /// 工具 -> 命令。依赖选中的那三件事在空选择集上无从谈起，返回 `None`；
    /// 调用点拿它同时决定按钮是否可用，不另写一套启用条件。
    ///
    /// 显示 / 隐藏收整批，定位只取主选中——两者的分别写在 `ModelAction` 上。
    fn action(self, selection: &Selection) -> Option<ModelAction> {
        Some(match self {
            Tool::Show => ModelAction::SetVisible {
                refnos: non_empty(selection)?,
                visible: true,
            },
            Tool::Hide => ModelAction::SetVisible {
                refnos: non_empty(selection)?,
                visible: false,
            },
            Tool::HideAll => ModelAction::HideAll,
            // 不按「有没有东西被藏着」置灰：绘制层手上没有这个数，Vm 里也没有，
            // 为一枚按钮的启用态往 Vm 上加一个全局标志不划算。按下去无事发生，
            // 与 `HideAll` 在空场景里按下去是同一种无害。
            Tool::ShowAll => ModelAction::ShowAll,
            Tool::Focus => ModelAction::Focus(selection.primary()?),
            Tool::Fit => ModelAction::FitAll,
        })
    }
}

fn non_empty(selection: &Selection) -> Option<Vec<RefU64>> {
    (!selection.is_empty()).then(|| selection.to_vec())
}

pub fn show(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm, cmds: &mut Vec<Cmd>) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    ui.painter()
        .add(gradient(rect, t.viewport_top, t.viewport_bottom));

    let Some(view) = vm.view3d else {
        note(ui, t, d, rect);
        return;
    };

    let uv = cover_uv(rect.size(), view.size);
    ui.painter().image(
        view.texture,
        rect,
        uv,
        if view.live {
            Color32::WHITE
        } else {
            Color32::from_white_alpha(PLACEHOLDER_ALPHA)
        },
    );
    let hud = hud(ui, t, d, rect, &view, vm.room_view.as_ref(), cmds);
    if !view.live {
        return;
    }
    // 渲染目标跟着视口走。宿主换纹理是异步的（还带防抖），在它换过来之前上面那句
    // `cover_uv` 照旧按当前纹理裁切——那几帧宁可少看见一圈，也不要把管子拉成椭圆。
    let wanted = wanted_texture_size(rect.size(), ui.ctx().pixels_per_point());
    if wanted != [view.size.x as u32, view.size.y as u32] {
        cmds.push(Cmd::ResizeViewport(wanted));
    }

    axis_labels(ui, t, d, rect, &view);
    // 浮层先画完再判拾取：工具栏、立方体、HUD 盖住的这几片是控件，不是模型。
    let bar = toolbar(ui, t, d, rect, vm, response.id, cmds);
    let cube = viewcube::show(ui, t, d, &view, rect, response.id, cmds);
    if response.clicked_by(egui::PointerButton::Primary)
        && let Some(pointer) = response.interact_pointer_pos()
    {
        // TODO(诊断): 拾取排查完删除。
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!(
            "[诊断] 视口收到左键点击 pointer={pointer:?} rect={rect:?} 落在工具栏={} 落在立方体={}",
            bar.contains(pointer),
            cube.contains(pointer)
        );
        // 点过视口，键盘就归视口。快捷键的开关是焦点，不是「鼠标在不在上面」。
        ui.memory_mut(|m| m.request_focus(response.id));
        if !bar.contains(pointer) && !cube.contains(pointer) && !hud.contains(pointer) {
            cmds.push(Cmd::PickViewport(pointer_texture_uv(
                rect.size(),
                view.size,
                pointer - rect.min,
            )));
        }
    }
    camera(ui, rect, view.size, [bar, cube, hud], &response, cmds);
    // 视口右键菜单：与模型树行菜单同一段构造器，内容一致（S13 稿）。作用对象
    // 是**当前选中**——视口的拾取是一次异步往返，右键当帧拿不到指针底下是谁，
    // 「先左键选中、再右键操作」与树上的习惯一致。没有选中就不摆空菜单，
    // 右键拖拽旋转不受影响（拖过门槛就不算点击）。
    if let Some(primary) = vm.selection.primary() {
        response.context_menu(|ui| {
            super::tree::element_menu(
                ui,
                t,
                d,
                primary,
                &vm.selection.to_vec(),
                &vm.rooms,
                true,
                vm.regen_busy,
                cmds,
                false,
            );
        });
    }
    // 焦点在命令行或属性输入框里时，敲 s / h / f 是在打字，不是在发命令。
    if response.has_focus() {
        for tool in Tool::BAR {
            if consume_exact(ui, tool.shortcut())
                && let Some(action) = tool.action(&vm.selection)
            {
                cmds.push(Cmd::Model(action));
            }
        }
    }
}

/// 相机手势用哪个键。左键未越过拖拽门槛时仍由拾取处理。
const CAMERA_BUTTONS: [(egui::PointerButton, CameraMotion); 3] = [
    (egui::PointerButton::Primary, CameraMotion::Pan),
    (egui::PointerButton::Middle, CameraMotion::Pan),
    (egui::PointerButton::Secondary, CameraMotion::Orbit),
];

/// 把 egui 的拖拽与滚轮翻成相机手势：左键 / 中键平移、右键旋转、滚轮缩放。
/// 左键未越过 egui 的拖拽门槛时仍是一次普通拾取。
///
/// 起手落在工具栏上的不算。`tool_btn` 只 `Sense::click()`，从按钮上按下再拖会
/// 穿到底下这块背景来——那是一次按废了的按键，不该顺势把相机拽走。反过来，
/// 已经转起来之后划过工具栏照常继续：一次手势在按下时定性，中途不改主意。
fn camera(
    ui: &Ui,
    view: Rect,
    texture: Vec2,
    blocked: [Rect; 3],
    response: &egui::Response,
    cmds: &mut Vec<Cmd>,
) {
    let uv = |pointer: egui::Pos2| pointer_texture_uv(view.size(), texture, pointer - view.min);
    let on_overlay = |pointer: egui::Pos2| blocked.iter().any(|r| r.contains(pointer));

    // egui 的拖拽不分按键，是 `drag_started_by` 回头看哪个键按着。两个键一起
    // 按下时两支都会报，所以取头一个就收手，别发出两次开始。
    let started = CAMERA_BUTTONS
        .into_iter()
        .find(|(button, _)| response.drag_started_by(*button));
    if let Some((_, motion)) = started
        && let Some(pointer) = ui.input(|i| i.pointer.press_origin())
        && !on_overlay(pointer)
    {
        // 操作过视口，键盘就归视口——转完直接敲 H 隐藏，不必再补一下点击。
        ui.memory_mut(|m| m.request_focus(response.id));
        cmds.push(Cmd::Camera(CameraGesture::Begin {
            motion,
            anchor: uv(pointer),
        }));
    }

    // 只有按着相机键的那段拖拽才算数。
    let holding_camera_button = CAMERA_BUTTONS
        .into_iter()
        .any(|(button, _)| response.dragged_by(button));
    let drag = response.drag_delta();
    if holding_camera_button && drag != Vec2::ZERO {
        cmds.push(Cmd::Camera(CameraGesture::Drag(texture_delta(
            view.size(),
            texture,
            drag,
        ))));
    }
    if CAMERA_BUTTONS
        .into_iter()
        .any(|(button, _)| response.drag_stopped_by(button))
    {
        cmds.push(Cmd::Camera(CameraGesture::End));
    }

    // `smooth_scroll_delta` 是 egui 给自定义滚动控件留的那个口子：滚动区读它、
    // 消费掉就清零，所以底下的工具栏滚起来时这里自然收不到。0.35 已经不再单独
    // 提供未平滑的那份，宿主的相机自己还有一档缩放平滑，两层叠着会略钝，
    // 真机上要是发黏，调的是宿主那档。
    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
    if scroll != 0.0
        && response.hovered()
        && let Some(pointer) = ui.input(|i| i.pointer.hover_pos())
        && !on_overlay(pointer)
    {
        // 这一份已经拿去缩放了，清掉，别让别的容器再消费一次。
        ui.input_mut(|i| i.smooth_scroll_delta.y = 0.0);
        cmds.push(Cmd::Camera(CameraGesture::Zoom {
            amount: scroll,
            anchor: uv(pointer),
        }));
    }
}

/// 三根世界轴的轴端标签（X 红 / Y 绿 / Z 蓝，S1-B 稿）。
///
/// 位置是宿主投影好的**纹理 UV**——世界点到屏幕要过相机，只有宿主算得了；
/// 这里只补「纹理 UV → 视口点」那一步等比裁切换算，与拾取的换算互为反函数，
/// truth 都压在 [`cover_uv`] 一处。轴尖被裁掉或在相机身后时宿主给 None，不画。
fn axis_labels(ui: &mut Ui, t: &Tokens, d: Density, view: Rect, vm3d: &View3dVm) {
    let uv_rect = cover_uv(view.size(), vm3d.size);
    for (axis, label) in ["X", "Y", "Z"].into_iter().enumerate() {
        let Some([u, v]) = vm3d.axis_labels[axis] else {
            continue;
        };
        let rel = vec2(
            (u - uv_rect.min.x) / uv_rect.width(),
            (v - uv_rect.min.y) / uv_rect.height(),
        );
        if !(0.0..=1.0).contains(&rel.x) || !(0.0..=1.0).contains(&rel.y) {
            continue;
        }
        ui.painter().text(
            view.min + rel * view.size(),
            Align2::CENTER_CENTER,
            label,
            Font::mono_meta(d),
            viewcube::axis_color(t, axis),
        );
    }
}

/// 视口左侧的竖向工具栏，返回它占住的矩形（调用点拿它把点击挡在拾取之外）。
///
/// 竖直居中，但上沿不越过左上角 HUD：两块浮层都贴着左边，视口一矮就叠上了。
fn toolbar(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    view: Rect,
    vm: &WorkbenchVm,
    viewport: Id,
    cmds: &mut Vec<Cmd>,
) -> Rect {
    let btn = d.px(30.0);
    let pad = d.px(4.0);
    let gap = d.px(4.0);
    let top = view.top() + d.px(HUD_INSET) * 2.0 + hud_h(d);
    let bottom = view.bottom() - d.px(12.0);
    let n = Tool::BAR.len() as f32;

    let room = (bottom - top - pad * 2.0).max(0.0);
    // 先收间距，再滚动。按钮宁可滚，也不画到视口外面去。
    let gap = if n * btn + (n - 1.0) * gap <= room {
        gap
    } else {
        0.0
    };
    let wanted = n * btn + (n - 1.0) * gap;
    let content = wanted.min(room);
    if content < btn {
        return Rect::NOTHING;
    }

    let size = vec2(btn + pad * 2.0, content + pad * 2.0);
    let center_y = ((top + bottom) / 2.0).clamp(top + size.y / 2.0, bottom - size.y / 2.0);
    let rect = Rect::from_center_size(
        pos2(view.left() + d.px(12.0) + size.x / 2.0, center_y),
        size,
    );

    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Center)),
    );
    egui::Frame::new()
        .fill(t.bg_elevated)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(radius::LG)
        .inner_margin(Margin::same(pad as i8))
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, gap);
            ui.set_width(btn);
            ui.set_height(content);
            if content < wanted {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| tool_buttons(ui, t, d, vm, viewport, cmds));
            } else {
                tool_buttons(ui, t, d, vm, viewport, cmds);
            }
        });
    rect
}

fn tool_buttons(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    viewport: Id,
    cmds: &mut Vec<Cmd>,
) {
    for tool in Tool::BAR {
        let action = tool.action(&vm.selection);
        let resp = ui
            .add_enabled(
                action.is_some(),
                widgets::tool_btn(t, d, tool.icon(), false),
            )
            .on_hover_text(tool.hint())
            .on_disabled_hover_text(tool.hint());
        if resp.clicked() {
            // 按过工具栏也算「在视口里操作」，键盘跟着回到视口。
            ui.memory_mut(|m| m.request_focus(viewport));
            if let Some(action) = action {
                cmds.push(Cmd::Model(action));
            }
        }
    }
}

/// 严格按修饰键匹配的一次性取键。
///
/// egui 的 `consume_shortcut` 走 `matches_logically`，多按一个 Shift 也算命中，
/// `H` 会把 `Shift+H` 一并吃掉——那样两个键位就只能靠调用顺序区分，谁挪一下就串。
fn consume_exact(ui: &Ui, shortcut: KeyboardShortcut) -> bool {
    ui.input_mut(|i| {
        let mut hit = false;
        i.events.retain(|event| {
            let is_match = matches!(
                event,
                egui::Event::Key { key, modifiers, pressed: true, .. }
                    if *key == shortcut.logical_key
                        && modifiers.matches_exact(shortcut.modifiers)
            );
            hit |= is_match;
            !is_match
        });
        hit
    })
}

/// 渲染目标的下限。不是为了好看：dock 分隔条拖到底时视口会变成零宽，
/// 零尺寸的纹理开不出来。
const MIN_TEXTURE_PX: f32 = 16.0;

/// 渲染目标的上限，取 wgpu 默认的 `max_texture_dimension_2d`。
/// 超了不是画糊一点，是创建纹理直接失败。
const MAX_TEXTURE_PX: f32 = 8192.0;

/// 视口这块矩形占多少物理像素——渲染目标就该开这么大。
fn wanted_texture_size(view: Vec2, pixels_per_point: f32) -> [u32; 2] {
    let px = |points: f32| {
        (points * pixels_per_point)
            .round()
            .clamp(MIN_TEXTURE_PX, MAX_TEXTURE_PX) as u32
    };
    [px(view.x), px(view.y)]
}

fn pointer_texture_uv(viewport: Vec2, texture: Vec2, pointer: Vec2) -> [f32; 2] {
    let uv = cover_uv(viewport, texture);
    let texture_uv = uv.min + pointer / viewport * uv.size();
    [texture_uv.x, texture_uv.y]
}

/// 竖直渐变。egui 没有渐变笔刷，四个顶点各自带色的三角网格就是标准做法。
fn gradient(rect: Rect, top: Color32, bottom: Color32) -> Shape {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(2, 1, 3);
    Shape::mesh(mesh)
}

/// 等比裁切铺满时纹理被放大的倍数（屏幕点数 / 纹理像素）。
/// 铺图与拖拽增量折算都要它，所以只算这一处。
fn cover_scale(view: egui::Vec2, texture: egui::Vec2) -> Option<f32> {
    (texture.x > 0.0 && texture.y > 0.0 && view.x > 0.0 && view.y > 0.0)
        .then(|| (view.x / texture.x).max(view.y / texture.y))
}

/// 等比裁切铺满（CSS 的 `object-fit: cover`）：算出该取纹理中间多大一块。
/// 两个方向上取到的比例都 ≤ 1，其中一个正好等于 1。
fn cover_uv(view: egui::Vec2, texture: egui::Vec2) -> Rect {
    let Some(scale) = cover_scale(view, texture) else {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    };
    let visible = vec2(view.x / (texture.x * scale), view.y / (texture.y * scale));
    Rect::from_center_size(pos2(0.5, 0.5), visible)
}

/// 屏幕上拖过的点数 -> 渲染纹理上走过的像素。
///
/// 相机要的是渲染目标自己那套像素，而屏幕上一点只对应纹理上 `1/scale` 个像素。
/// 不折算的话，同一段手势在拉宽的视口里转得多、在压窄的视口里转得少——手感
/// 会跟着 dock 分隔条变。
fn texture_delta(view: egui::Vec2, texture: egui::Vec2, delta: Vec2) -> [f32; 2] {
    let scale = cover_scale(view, texture).unwrap_or(1.0);
    [delta.x / scale, delta.y / scale]
}

/// HUD 距视口边的距离（S1 的「HUD状态」规格）。工具栏要按它算自己的上界，
/// 所以从 `hud` 里提出来，不让两处各写一个 14。
const HUD_INSET: f32 = 14.0;

fn hud_h(d: Density) -> f32 {
    d.px(26.0)
}

/// 格距读数：真实长度换成人念得出的单位。
///
/// 宿主的档位是 1/2/5×10ⁿ 毫米，永远落在整数上，所以不留小数位——「1 m」比
/// 「1.0 m」更像一句话。独立壳没有网格（`live` 为假、读数为 0），返回 None
/// 让这一段连同它的分隔线一起消失，而不是显示一个「一格 0 mm」。
fn grid_cell(vm3d: &View3dVm) -> Option<String> {
    let mm = vm3d.grid_cell_mm;
    if !vm3d.live || !mm.is_finite() || mm <= 0.0 {
        return None;
    }
    Some(if mm >= 1_000_000.0 {
        format!("{} km", (mm / 1_000_000.0).round() as i64)
    } else if mm >= 1_000.0 {
        format!("{} m", (mm / 1_000.0).round() as i64)
    } else {
        format!("{} mm", mm.round() as i64)
    })
}

/// 左上角的 HUD（S1 的「HUD状态」位置与规格：距边 14、高 26、bg-elevated + 1px 边）。
///
/// 房间视图进行中时多出一段徽章（S13-B 稿的「房间视图 · R301」）和「退出」
/// 按钮。返回占住的矩形：HUD 从此有了交互，从它上面起手的点击 / 拖拽不该
/// 穿到拾取与相机上——与工具栏、立方体同一条规矩。
fn hud(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    view: Rect,
    vm3d: &View3dVm,
    room: Option<&RoomViewVm>,
    cmds: &mut Vec<Cmd>,
) -> Rect {
    let live = vm3d.live;
    let inset = d.px(HUD_INSET);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(
                view.min + vec2(inset, inset),
                view.max - vec2(inset, inset),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Min)),
    );
    let frame = egui::Frame::new()
        .fill(t.bg_elevated)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::symmetric(d.px(10.0) as i8, 0))
        .show(&mut child, |ui| {
            ui.set_height(hud_h(d));
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = d.px(10.0);
                let (dot, _) = ui.allocate_exact_size(egui::Vec2::splat(d.px(6.0)), Sense::hover());
                ui.painter().circle_filled(
                    dot.center(),
                    d.px(3.0),
                    if live { t.success } else { t.text_muted },
                );
                ui.label(
                    RichText::new(if live {
                        "实时模型 · Bevy"
                    } else {
                        "占位纹理 · 未接渲染器"
                    })
                    .font(Font::mono_meta(d))
                    .color(t.text_secondary),
                );
                let (div, _) = ui.allocate_exact_size(vec2(1.0, d.px(12.0)), Sense::hover());
                ui.painter().rect_filled(div, 0, t.border);
                ui.label(
                    RichText::new(if live {
                        "相机渲染目标"
                    } else {
                        "实时视口在 M3 接回 Bevy"
                    })
                    .font(Font::mono_meta(d))
                    .color(t.text_muted),
                );
                // 比例读数：地面网格一格多长。没有它，网格与三轴只是一组好看的
                // 线——看得见疏密，读不出尺寸，而它们恰恰是视口里唯一的长度参照。
                if let Some(cell) = grid_cell(vm3d) {
                    let (div, _) = ui.allocate_exact_size(vec2(1.0, d.px(12.0)), Sense::hover());
                    ui.painter().rect_filled(div, 0, t.border);
                    ui.label(
                        RichText::new(format!("一格 {cell}"))
                            .font(Font::mono_meta(d))
                            .color(t.text_muted),
                    )
                    .on_hover_text("地面网格的格距，随相机远近换档");
                }
                if let Some(room) = room {
                    let (div, _) = ui.allocate_exact_size(vec2(1.0, d.px(12.0)), Sense::hover());
                    ui.painter().rect_filled(div, 0, t.border);
                    ui.label(
                        RichText::new(format!(
                            "{} 房间视图 · {} · 成员 {}",
                            ph::DOOR_OPEN,
                            room.room_num,
                            room.member_count
                        ))
                        .font(Font::mono_meta(d))
                        .color(t.accent_strong),
                    );
                    if ui
                        .add(widgets::chip(t, d, "退出", false))
                        .on_hover_text("退出房间视图，恢复隔离前的可见性")
                        .clicked()
                    {
                        cmds.push(Cmd::Model(ModelAction::ExitIsolate));
                    }
                }
            });
        });
    frame.response.rect
}

/// 没有纹理时的错误态。
///
/// 三维视图只有这一态：占位图是 App 启动时同步载入的，没有「加载中」，
/// 也没有「未初始化」——`vm.view3d` 为 None 只可能是载入失败。M3 接回 Bevy 后
/// 相机还没给出渲染目标的那一段才会有真正的未初始化，到时候再加。
///
/// 底色不铺：渐变已经画在下面了，`pane_note` 的 bg-panel 会把它盖掉，所以这里
/// 用一个背景透明的子 Ui 套住它。
fn note(ui: &mut Ui, t: &Tokens, d: Density, view: Rect) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(view));
    let transparent = Tokens {
        bg_panel: Color32::TRANSPARENT,
        ..*t
    };
    widgets::pane_note(
        &mut child,
        &transparent,
        d,
        widgets::PaneNote {
            state: widgets::PaneState::Error,
            icon: ph::IMAGE_BROKEN,
            text: "占位纹理未能载入",
            detail: Some("assets/textures 下的图缺了或读不了"),
            retry: false,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::View3dVm;

    /// 在一个真的 `egui::Context` 上跑视口，把合成输入喂进去、收命令出来。
    ///
    /// 先空跑一帧再送事件：egui 的命中判定用的是**上一帧**登记的控件矩形，
    /// 头一帧视口这块矩形还不存在，点在哪儿都不算数。
    fn viewport_cmds(frames: Vec<Vec<egui::Event>>) -> Vec<Cmd> {
        viewport_cmds_with(vec2(1600.0, 900.0), frames)
    }

    /// 同上，纹理尺寸可选——「渲染目标跟着视口走」那条要拿它和视口尺寸对照。
    fn viewport_cmds_with(texture: Vec2, frames: Vec<Vec<egui::Event>>) -> Vec<Cmd> {
        viewport_cmds_on(live_vm(texture), frames)
    }

    /// 一个接着实时渲染器的工作台 Vm，测试要改别的字段（如 `room_view`）就在
    /// 返回值上继续摆。
    fn live_vm(texture: Vec2) -> WorkbenchVm {
        WorkbenchVm {
            view3d: Some(View3dVm {
                texture: egui::TextureId::User(0),
                size: texture,
                live: true,
                measurement_active: false,
                camera_rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                axis_labels: [None; 3],
                grid_cell_mm: 1_000.0,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn the_hud_reads_the_cell_out_in_human_units() {
        let mut vm3d = live_vm(vec2(1600.0, 900.0)).view3d.expect("实时视口");
        // 宿主的档位是 1/2/5×10ⁿ 毫米，所以这几个就是读数会遇到的全部形状。
        for (mm, text) in [
            (1.0, "1 mm"),
            (20.0, "20 mm"),
            (500.0, "500 mm"),
            (1_000.0, "1 m"),
            (2_000.0, "2 m"),
            (200_000.0, "200 m"),
            (1_000_000.0, "1 km"),
        ] {
            vm3d.grid_cell_mm = mm;
            assert_eq!(grid_cell(&vm3d).as_deref(), Some(text), "{mm} mm 的读数");
        }
        // 独立壳没有网格，这一段连同它的分隔线一起消失，而不是显示「一格 0 mm」。
        vm3d.live = false;
        assert_eq!(grid_cell(&vm3d), None);
        vm3d.live = true;
        vm3d.grid_cell_mm = 0.0;
        assert_eq!(grid_cell(&vm3d), None);
    }

    /// 同上，工作台 Vm 由调用方给。
    fn viewport_cmds_on(vm: WorkbenchVm, frames: Vec<Vec<egui::Event>>) -> Vec<Cmd> {
        let ctx = egui::Context::default();
        let t = crate::style::theme_tokens::current();
        let d = Density::Standard;
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 700.0));

        let mut frame = |events: Vec<egui::Event>| {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            });
            let mut cmds = Vec::new();
            // 用 Area 而不是 CentralPanel：宿主装配工作台时用的就是 Area，
            // 让这里的取尺寸方式和真机一致。
            egui::Area::new("test-viewport".into())
                .fixed_pos(screen.min)
                .show(&ctx, |ui| {
                    ui.set_min_size(screen.size());
                    ui.set_max_size(screen.size());
                    show(ui, &t, d, &vm, &mut cmds);
                });
            let _ = ctx.end_pass();
            cmds
        };
        // 空跑一帧登记控件矩形，之后每一批事件一帧。
        frame(Vec::new());
        frames.into_iter().flat_map(&mut frame).collect()
    }

    /// 一次点击摊成三帧：移过去、按下、抬起。真机上按下与抬起本来就跨帧，
    /// 挤进一帧 egui 判不出这是一次点击。
    fn click_at(pointer: egui::Pos2) -> Vec<Vec<egui::Event>> {
        let button = |pressed| {
            vec![egui::Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }]
        };
        vec![
            vec![egui::Event::PointerMoved(pointer)],
            button(true),
            button(false),
        ]
    }

    /// 在视口里点一下，得发得出拾取命令。
    ///
    /// 这一半只有真机上有人拿鼠标点才走得到，坏了看到的只是「点了没反应」，
    /// 分不清是命令没发出来、还是宿主没接住。钉在这儿，下回能分得清。
    #[test]
    fn clicking_the_viewport_emits_a_pick() {
        let cmds = viewport_cmds(click_at(pos2(600.0, 350.0)));
        let picks: Vec<_> = cmds
            .iter()
            .filter_map(|cmd| match cmd {
                Cmd::PickViewport(uv) => Some(*uv),
                _ => None,
            })
            .collect();
        assert_eq!(picks.len(), 1, "视口里点一下没发出拾取命令：{cmds:?}");
        let [u, v] = picks[0];
        assert!(
            (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v),
            "拾取 UV 落到纹理外面去了：{:?}",
            picks[0]
        );
    }

    /// 左键 / 中键拖 = 平移，右键拖 = 旋转。
    #[test]
    fn left_and_middle_pan_while_right_orbits() {
        let motion_of = |button| {
            let from = pos2(600.0, 350.0);
            let to = pos2(640.0, 350.0);
            let press = |pos, pressed| {
                vec![egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers: Modifiers::NONE,
                }]
            };
            viewport_cmds(vec![
                vec![egui::Event::PointerMoved(from)],
                press(from, true),
                vec![egui::Event::PointerMoved(to)],
                press(to, false),
            ])
            .into_iter()
            .find_map(|cmd| match cmd {
                Cmd::Camera(CameraGesture::Begin { motion, .. }) => Some(motion),
                _ => None,
            })
        };

        assert_eq!(
            motion_of(egui::PointerButton::Primary),
            Some(CameraMotion::Pan)
        );
        assert_eq!(
            motion_of(egui::PointerButton::Middle),
            Some(CameraMotion::Pan)
        );
        assert_eq!(
            motion_of(egui::PointerButton::Secondary),
            Some(CameraMotion::Orbit)
        );
    }

    /// 点在工具栏上不该穿成一次拾取——「按显示按钮的同时选中了背后的管子」。
    #[test]
    fn clicking_the_toolbar_does_not_pick_through_it() {
        // 工具栏贴着视口左缘，距左 12、宽 30 加两侧 4 的内边距。
        let cmds = viewport_cmds(click_at(pos2(30.0, 350.0)));
        assert!(
            !cmds.iter().any(|cmd| matches!(cmd, Cmd::PickViewport(_))),
            "点在工具栏上却穿成了拾取：{cmds:?}"
        );
    }

    /// 房间视图徽章带着「退出」按钮，HUD 从此是控件不是模型——点在它上面
    /// 不该穿成拾取（与工具栏、立方体同一条规矩）。
    #[test]
    fn clicking_the_hud_does_not_pick_through_it() {
        let mut vm = live_vm(vec2(1600.0, 900.0));
        vm.room_view = Some(RoomViewVm {
            room: RefU64(7),
            room_num: "R301".into(),
            member_count: 128,
        });
        // HUD 距边 14、高 26，(20, 27) 落在它的框里。
        let cmds = viewport_cmds_on(vm, click_at(pos2(20.0, 27.0)));
        assert!(
            !cmds.iter().any(|cmd| matches!(cmd, Cmd::PickViewport(_))),
            "点在 HUD 上却穿成了拾取：{cmds:?}"
        );
    }

    /// 点在视图立方体上发出的是视角跳转，不是模型拾取。恒等姿态下立方体只露
    /// 「前」面，点它的中心格 = 请求北向正视（世界系 forward -Z、up +Y）。
    /// 方向错半个符号，立方体会把人带到背面去，所以连数值一起钉死。
    #[test]
    fn clicking_the_viewcube_snaps_the_view_instead_of_picking() {
        // 立方体占位：距右 14、边长 88，屏幕 1200 宽 → 中心 (1142, 58)。
        let cmds = viewport_cmds(click_at(pos2(1142.0, 58.0)));
        assert!(
            !cmds.iter().any(|cmd| matches!(cmd, Cmd::PickViewport(_))),
            "点在立方体上却穿成了拾取：{cmds:?}"
        );
        let snap = cmds.iter().find_map(|cmd| match cmd {
            Cmd::SnapView { forward, up, fit } => Some((*forward, *up, *fit)),
            _ => None,
        });
        assert_eq!(
            snap,
            Some(([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], false)),
            "点「前」面中心该请求北向视角：{cmds:?}"
        );
    }

    /// Home 键在立方体左上角：点它要求「回等距初始视角 + 拉到全景」，fit 必须为真
    /// ——这正是它与普通面点击的分别，丢了 fit 它就退化成一枚普通角区。
    #[test]
    fn the_home_button_snaps_and_fits() {
        let cmds = viewport_cmds(click_at(pos2(1106.0, 22.0)));
        let fit = cmds.iter().find_map(|cmd| match cmd {
            Cmd::SnapView { fit, .. } => Some(*fit),
            _ => None,
        });
        assert_eq!(fit, Some(true), "Home 键没有带上 fit：{cmds:?}");
    }

    /// 报的是物理像素：150% 缩放下要的纹理比点数大一半，不然铺到屏幕上又是一次
    /// 放大插值。上下限两头都夹住——零宽的视口开不出纹理，越过 wgpu 上限是创建
    /// 当场失败，不是画糊一点。
    #[test]
    fn texture_size_follows_dpi_and_stays_within_limits() {
        assert_eq!(wanted_texture_size(vec2(800.0, 450.0), 1.5), [1200, 675]);
        assert_eq!(wanted_texture_size(vec2(0.0, 700.0), 1.0), [16, 700]);
        assert_eq!(wanted_texture_size(vec2(9000.0, 700.0), 1.0), [8192, 700]);
    }

    /// 视口按自己占的物理像素要纹理，要到了就不再吭声。
    ///
    /// 少了前一半，渲染目标永远停在开机写死的那个尺寸，面板一换形状画面就被裁掉
    /// 一圈；少了后一半，这条命令每帧都在发，宿主那道防抖窗口永远够不着。
    #[test]
    fn the_viewport_asks_for_a_render_target_its_own_size() {
        let asked = |texture: Vec2| {
            viewport_cmds_with(texture, vec![Vec::new()])
                .into_iter()
                .find_map(|cmd| match cmd {
                    Cmd::ResizeViewport(size) => Some(size),
                    _ => None,
                })
        };
        // 测试上下文是 1 点 = 1 像素，视口占满 1200×700 的屏幕。
        assert_eq!(asked(vec2(1600.0, 900.0)), Some([1200, 700]));
        assert_eq!(asked(vec2(1200.0, 700.0)), None, "尺寸已经对上了还在要");
    }

    #[test]
    fn pointer_uv_includes_cover_crop() {
        assert_eq!(
            pointer_texture_uv(vec2(200.0, 100.0), vec2(100.0, 100.0), Vec2::ZERO),
            [0.0, 0.25]
        );
    }

    /// 拖拽增量按同一个裁切倍数折回纹理像素：视口是纹理的两倍宽时，
    /// 屏幕上划 20 点等于纹理上走 10 像素。同一段手势换个视口大小
    /// 转过的角度得一样，这条就是钉这件事的。
    #[test]
    fn drag_delta_is_measured_in_texture_pixels() {
        assert_eq!(
            texture_delta(vec2(200.0, 200.0), vec2(100.0, 100.0), vec2(20.0, -6.0)),
            [10.0, -3.0]
        );
        // 纹理尺寸还没到手时不该把增量算成 NaN 或者零，原样交出去。
        assert_eq!(
            texture_delta(vec2(200.0, 200.0), Vec2::ZERO, vec2(20.0, -6.0)),
            [20.0, -6.0]
        );
    }

    /// 工具栏与树菜单发的是同一批命令，映射错了两处一起错，所以钉住它。
    #[test]
    fn tools_map_to_model_actions() {
        let refno = RefU64(42);
        let mapped = Tool::BAR.map(|tool| tool.action(&Selection::single(refno)));
        assert_eq!(
            mapped,
            [
                Some(ModelAction::SetVisible {
                    refnos: vec![refno],
                    visible: true
                }),
                Some(ModelAction::SetVisible {
                    refnos: vec![refno],
                    visible: false
                }),
                Some(ModelAction::HideAll),
                Some(ModelAction::ShowAll),
                Some(ModelAction::Focus(refno)),
                Some(ModelAction::FitAll),
            ]
        );
    }

    /// 多选下显示 / 隐藏收整批，定位只带相机去主选中那一个。
    /// 这两种作用域的分别正是 `ModelAction` 上那段注释说的事，用例把它钉死。
    #[test]
    fn visibility_takes_the_batch_while_focus_takes_the_primary() {
        let mut selection = Selection::single(RefU64(1));
        selection.toggle(RefU64(2));
        assert_eq!(
            Tool::Hide.action(&selection),
            Some(ModelAction::SetVisible {
                refnos: vec![RefU64(1), RefU64(2)],
                visible: false
            })
        );
        assert_eq!(
            Tool::Focus.action(&selection),
            Some(ModelAction::Focus(RefU64(2)))
        );
    }

    /// 空选择集时，依赖选中的三件事必须自己说「做不了」——
    /// 按钮的禁用态就是照这个来的，两边不能各判各的。
    ///
    /// 反过来，`HideAll` / `ShowAll` / `FitAll` 三个全局动作在空选择集下照样成立：
    /// 它们作用于整个场景，跟选中什么没关系。
    #[test]
    fn per_element_tools_need_a_selection() {
        assert_eq!(
            Tool::BAR.map(|tool| tool.action(&Selection::default())),
            [
                None,
                None,
                Some(ModelAction::HideAll),
                Some(ModelAction::ShowAll),
                None,
                Some(ModelAction::FitAll),
            ]
        );
    }

    /// 所选与全部只差一个 Shift，键位表里必须是两两互不相等的记录。
    ///
    /// 按整表查而不是只查那一对：撞键的后果是按下去两条命令一起发，而 egui 的
    /// `consume_shortcut` 谁先问谁得手，光看键位表看不出来。加第七枚工具时这条
    /// 会替你发现撞车。
    #[test]
    fn every_tool_has_its_own_shortcut() {
        for (i, a) in Tool::BAR.iter().enumerate() {
            for b in &Tool::BAR[i + 1..] {
                assert_ne!(a.shortcut(), b.shortcut(), "{a:?} 与 {b:?} 撞键了");
            }
        }
        assert!(Tool::HideAll.shortcut().modifiers.shift);
        assert!(!Tool::Hide.shortcut().modifiers.shift);
        assert!(Tool::ShowAll.shortcut().modifiers.shift);
        assert!(!Tool::Show.shortcut().modifiers.shift);
    }
}
