//! plant-ui 共用字体注册。

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

pub const MEDIUM: &str = "puhui-medium";
pub const SEMIBOLD: &str = "puhui-semibold";

const REGULAR_KEY: &str = "puhui-regular";

fn font_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts")
}

fn load(file: &str, warnings: &mut Vec<String>) -> Option<Vec<u8>> {
    let path = font_dir().join(file);
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(err) => {
            warnings.push(format!(
                "字体缺失，回退到 egui 默认字体：{} ({err})",
                path.display()
            ));
            None
        }
    }
}

pub fn definitions() -> (FontDefinitions, Vec<String>) {
    let mut fonts = FontDefinitions::default();
    let mut warnings = Vec::new();
    let faces = [
        (REGULAR_KEY, "AlibabaPuHuiTi-2-55-Regular.ttf"),
        (MEDIUM, "AlibabaPuHuiTi-2-65-Medium.ttf"),
        (SEMIBOLD, "AlibabaPuHuiTi-2-75-SemiBold.ttf"),
    ];

    let mut loaded = Vec::new();
    for (key, file) in faces {
        if let Some(bytes) = load(file, &mut warnings) {
            fonts
                .font_data
                .insert(key.to_owned(), Arc::new(FontData::from_owned(bytes)));
            loaded.push(key);
        }
    }

    if loaded.contains(&REGULAR_KEY) {
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, REGULAR_KEY.to_owned());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push(REGULAR_KEY.to_owned());
    }

    for key in [MEDIUM, SEMIBOLD] {
        if !loaded.contains(&key) {
            continue;
        }
        let mut chain = vec![key.to_owned()];
        if loaded.contains(&REGULAR_KEY) {
            chain.push(REGULAR_KEY.to_owned());
        }
        chain.extend(
            fonts
                .families
                .get(&FontFamily::Proportional)
                .cloned()
                .unwrap_or_default(),
        );
        fonts
            .families
            .insert(FontFamily::Name(key.into()), dedup(chain));
    }

    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    (fonts, warnings)
}

fn dedup(chain: Vec<String>) -> Vec<String> {
    let mut seen = Vec::with_capacity(chain.len());
    for item in chain {
        if !seen.contains(&item) {
            seen.push(item);
        }
    }
    seen
}
