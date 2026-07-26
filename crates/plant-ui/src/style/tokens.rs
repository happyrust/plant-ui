//! 设计令牌：颜色、密度、圆角、间距。
//!
//! 唯一真实来源是 `design/rs-plant3d-ui.pen` 里的变量表，改这里之前先改设计文件。

use egui::Color32;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tokens {
    pub dark: bool,

    pub bg_app: Color32,
    pub bg_chrome: Color32,
    pub bg_panel: Color32,
    pub bg_header: Color32,
    pub bg_elevated: Color32,
    pub bg_input: Color32,
    pub bg_hover: Color32,

    pub border: Color32,
    pub border_strong: Color32,

    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,

    pub accent: Color32,
    pub accent_strong: Color32,
    pub accent_ink: Color32,
    pub accent_bg: Color32,

    pub danger: Color32,
    pub danger_bg: Color32,
    pub warn: Color32,
    pub warn_bg: Color32,
    pub success: Color32,
    pub success_bg: Color32,

    pub viewport_top: Color32,
    pub viewport_bottom: Color32,
}

impl Tokens {
    pub const fn dark() -> Self {
        Self {
            dark: true,
            bg_app: rgb(0x14, 0x18, 0x1C),
            bg_chrome: rgb(0x0F, 0x13, 0x17),
            bg_panel: rgb(0x1A, 0x20, 0x26),
            bg_header: rgb(0x1F, 0x26, 0x2D),
            bg_elevated: rgb(0x24, 0x2D, 0x35),
            bg_input: rgb(0x10, 0x15, 0x1A),
            bg_hover: rgb(0x23, 0x2C, 0x34),
            border: rgb(0x2A, 0x33, 0x3B),
            border_strong: rgb(0x3A, 0x45, 0x4F),
            text_primary: rgb(0xE8, 0xED, 0xF2),
            text_secondary: rgb(0xA9, 0xB7, 0xC3),
            text_muted: rgb(0x84, 0x94, 0xA1),
            accent: rgb(0x74, 0xA7, 0xCC),
            accent_strong: rgb(0x9A, 0xC4, 0xE2),
            accent_ink: rgb(0x0B, 0x11, 0x16),
            accent_bg: rgb(0x1E, 0x32, 0x3F),
            danger: rgb(0xF0, 0x88, 0x8A),
            danger_bg: rgb(0x3A, 0x21, 0x24),
            warn: rgb(0xE9, 0xB4, 0x4C),
            warn_bg: rgb(0x33, 0x2A, 0x18),
            success: rgb(0x5C, 0xBF, 0x82),
            success_bg: rgb(0x16, 0x30, 0x1F),
            viewport_top: rgb(0x23, 0x2F, 0x3A),
            viewport_bottom: rgb(0x0E, 0x13, 0x18),
        }
    }

    pub const fn light() -> Self {
        Self {
            dark: false,
            bg_app: rgb(0xE9, 0xEC, 0xF0),
            bg_chrome: rgb(0xFF, 0xFF, 0xFF),
            bg_panel: rgb(0xFF, 0xFF, 0xFF),
            bg_header: rgb(0xF3, 0xF5, 0xF8),
            bg_elevated: rgb(0xFF, 0xFF, 0xFF),
            bg_input: rgb(0xF3, 0xF5, 0xF8),
            bg_hover: rgb(0xED, 0xF1, 0xF5),
            border: rgb(0xDD, 0xE3, 0xE9),
            border_strong: rgb(0xC4, 0xCD, 0xD6),
            text_primary: rgb(0x16, 0x20, 0x2A),
            text_secondary: rgb(0x4C, 0x5A, 0x67),
            text_muted: rgb(0x64, 0x70, 0x7C),
            accent: rgb(0x4C, 0x73, 0x92),
            accent_strong: rgb(0x3A, 0x5E, 0x7B),
            accent_ink: rgb(0xFF, 0xFF, 0xFF),
            accent_bg: rgb(0xE1, 0xEB, 0xF3),
            danger: rgb(0xC4, 0x2B, 0x2B),
            danger_bg: rgb(0xFB, 0xE6, 0xE6),
            warn: rgb(0x9A, 0x62, 0x06),
            warn_bg: rgb(0xFB, 0xF0, 0xDA),
            success: rgb(0x1B, 0x7A, 0x44),
            success_bg: rgb(0xE1, 0xF2, 0xE7),
            viewport_top: rgb(0xDC, 0xE7, 0xF2),
            viewport_bottom: rgb(0xF4, 0xF7, 0xFA),
        }
    }

    pub const fn of(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    /// 状态色与其底色成对出现，避免调用点自己配对配错。
    pub const fn status(&self, s: Status) -> (Color32, Color32) {
        match s {
            Status::Info => (self.accent, self.accent_bg),
            Status::Warn => (self.warn, self.warn_bg),
            Status::Error => (self.danger, self.danger_bg),
            Status::Success => (self.success, self.success_bg),
            Status::Neutral => (self.text_muted, self.bg_hover),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Info,
    Warn,
    Error,
    Success,
    Neutral,
}

/// 界面密度。基准字号同时决定所有控件高度，改这一个值整套界面跟着缩放。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    Compact,
    Standard,
    Relaxed,
}

impl Density {
    pub const ALL: [Density; 3] = [Density::Compact, Density::Standard, Density::Relaxed];

    pub const fn base(self) -> f32 {
        match self {
            Density::Compact => 12.0,
            Density::Standard => 13.0,
            Density::Relaxed => 15.0,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Density::Compact => "紧凑",
            Density::Standard => "标准",
            Density::Relaxed => "宽松",
        }
    }

    /// 设计稿按基准 13 排版，其余尺寸都是相对它缩放出来的。
    pub fn scale(self) -> f32 {
        self.base() / 13.0
    }

    /// 把设计稿上以 13 为基准量得的像素值换算到当前密度。
    pub fn px(self, at_base_13: f32) -> f32 {
        (at_base_13 * self.scale()).round()
    }

    pub fn micro(self) -> f32 {
        self.px(10.0)
    }
    pub fn meta(self) -> f32 {
        self.px(11.0)
    }
    pub fn body(self) -> f32 {
        self.px(12.0)
    }
    pub fn label_size(self) -> f32 {
        self.base()
    }
    pub fn title(self) -> f32 {
        self.px(14.0)
    }
    pub fn page(self) -> f32 {
        self.px(18.0)
    }

    pub fn row_h(self) -> f32 {
        self.px(26.0)
    }
    pub fn tab_h(self) -> f32 {
        self.px(32.0)
    }
    pub fn title_bar_h(self) -> f32 {
        self.px(38.0)
    }
    pub fn command_bar_h(self) -> f32 {
        self.px(40.0)
    }
    pub fn status_bar_h(self) -> f32 {
        self.px(26.0)
    }
    pub fn btn_h(self) -> f32 {
        self.px(30.0)
    }
    pub fn input_h(self) -> f32 {
        self.px(28.0)
    }
}

pub mod radius {
    pub const SM: u8 = 3;
    pub const MD: u8 = 6;
    pub const LG: u8 = 10;
}

pub mod space {
    pub const S1: f32 = 4.0;
    pub const S2: f32 = 8.0;
    pub const S3: f32 = 12.0;
    pub const S4: f32 = 16.0;
}
