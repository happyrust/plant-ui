//! PDMS / E3D 3.1 颜色表（E3D 外观移植计划阶段一）。
//!
//! 由 plant3d-web `src/viewer/e3dLook/pdmsColourTable.ts`（2026-09-20 逆向 `core.dll FWSCOL` +
//! `DruidNet.dll ColourTableManager`，365 项）于 2026-10-22 生成，**勿手改**：要改去改来源再重新生成。
//!
//! - 1..16     基本色名 grey red orange yellow green cyan blue violet brown white pink mauve turquoise indigo black magenta
//! - 17..272   256 项 PDMS 调色板（其中 61 个槽位被字典色名覆盖，如 268 darkgrey / 270 grey / 271 lightgrey）
//! - 296       固定辅助色；298..303 lightyellow / white / midnight / lightyellow / gold
//! - 305..365  61 个字典色按字典顺序（305 black … 310 lightgrey … 365 stateblue）
//!
//! hex 是 `ColourTableManager` 按 `(int)(x·255)` 截断后的字节，即 E3D 屏幕上真正显示的 sRGB 字节；
//! 本模块的所有颜色都按**sRGB 字节**理解，不做线性化——与 sglDx11 直写 UNORM 目标的口径一致。
//!
//! PDMS 色名与 CSS 不是一套：PDMS `lightgrey` = #bdbdbd（CSS #d3d3d3）、`grey` = #828282（CSS gray #808080）、
//! `red` = #bc0000、`orange` = #ffbe00。所以这里**不认** CSS 名。

/// 一条颜色：PDMS 颜色号（1 起）、颜色名（无名槽位为空串）、`0xRRGGBB`。
pub type PdmsColourEntry = (u16, &'static str, u32);

pub const PDMS_COLOUR_TABLE: [PdmsColourEntry; 365] = [
    (1, "grey", 0x828282),
    (2, "red", 0xbc0000),
    (3, "orange", 0xffbe00),
    (4, "yellow", 0xffff00),
    (5, "green", 0x00bc00),
    (6, "cyan", 0x00ffff),
    (7, "blue", 0x0000ff),
    (8, "violet", 0xffaaea),
    (9, "brown", 0x683400),
    (10, "white", 0xffffff),
    (11, "pink", 0xbc7d8d),
    (12, "mauve", 0x5f0081),
    (13, "turquoise", 0x00bcbc),
    (14, "indigo", 0x340068),
    (15, "black", 0x000000),
    (16, "magenta", 0xff00ff),
    (17, "black", 0x000000),
    (18, "brightred", 0xff0000),
    (19, "yellow", 0xffff00),
    (20, "", 0x00ff00),
    (21, "cyan", 0x00ffff),
    (22, "blue", 0x0000ff),
    (23, "magenta", 0xff00ff),
    (24, "white", 0xffffff),
    (25, "", 0x414141),
    (26, "", 0x808080),
    (27, "", 0xff0000),
    (28, "", 0xffaaaa),
    (29, "red", 0xbc0000),
    (30, "indianred", 0xbc7d7d),
    (31, "", 0x810000),
    (32, "", 0x815555),
    (33, "", 0x680000),
    (34, "", 0x684545),
    (35, "", 0x4f0000),
    (36, "", 0x4f3535),
    (37, "tomato", 0xff3e00),
    (38, "salmon", 0xffbeaa),
    (39, "coralred", 0xbc2d00),
    (40, "", 0xbc8d7d),
    (41, "", 0x811f00),
    (42, "", 0x815f55),
    (43, "", 0x681800),
    (44, "", 0x684e45),
    (45, "", 0x4f1300),
    (46, "", 0x4f3a35),
    (47, "orangered", 0xff7e00),
    (48, "wheat", 0xffd3aa),
    (49, "tan", 0xbc8d7d),
    (50, "", 0xbc9d7d),
    (51, "sienna", 0x814000),
    (52, "", 0x816b55),
    (53, "brown", 0x683400),
    (54, "", 0x685545),
    (55, "chocolate", 0x4f2700),
    (56, "", 0x4f4235),
    (57, "orange", 0xffbe00),
    (58, "lightgold", 0xffeaaa),
    (59, "sandybrown", 0xbc8d00),
    (60, "beige", 0xbcac7d),
    (61, "", 0x815f00),
    (62, "", 0x817655),
    (63, "", 0x684e00),
    (64, "", 0x685f45),
    (65, "darkbrown", 0x4f3a00),
    (66, "", 0x4f4835),
    (67, "", 0xffff00),
    (68, "ivory", 0xffffaa),
    (69, "", 0xbcbc00),
    (70, "", 0xbcbc7d),
    (71, "", 0x818100),
    (72, "khaki", 0x818155),
    (73, "", 0x686800),
    (74, "", 0x686845),
    (75, "", 0x4f4f00),
    (76, "", 0x4f4f35),
    (77, "", 0xbeff00),
    (78, "", 0xeaffaa),
    (79, "yellowgreen", 0x8dbc00),
    (80, "", 0xacbc7d),
    (81, "", 0x5f8100),
    (82, "", 0x768155),
    (83, "", 0x4e6800),
    (84, "", 0x5f6845),
    (85, "", 0x3a4f00),
    (86, "", 0x484f35),
    (87, "", 0x7eff00),
    (88, "", 0xd3ffaa),
    (89, "", 0x5ebc00),
    (90, "", 0x9dbc7d),
    (91, "", 0x408100),
    (92, "", 0x6b8155),
    (93, "", 0x346800),
    (94, "", 0x556845),
    (95, "", 0x274f00),
    (96, "", 0x424f35),
    (97, "limegreen", 0x3eff00),
    (98, "", 0xbeffaa),
    (99, "", 0x2dbc00),
    (100, "", 0x8dbc7d),
    (101, "forestgreen", 0x1f8100),
    (102, "", 0x5f8155),
    (103, "", 0x186800),
    (104, "", 0x4e6845),
    (105, "", 0x134f00),
    (106, "", 0x3a4f35),
    (107, "", 0x00ff00),
    (108, "", 0xaaffaa),
    (109, "green", 0x00bc00),
    (110, "", 0x7dbc7d),
    (111, "", 0x008100),
    (112, "", 0x558155),
    (113, "", 0x006800),
    (114, "", 0x456845),
    (115, "", 0x004f00),
    (116, "", 0x354f35),
    (117, "", 0x00ff3e),
    (118, "", 0xaaffbe),
    (119, "", 0x00bc2d),
    (120, "", 0x7dbc8d),
    (121, "", 0x00811f),
    (122, "", 0x55815f),
    (123, "", 0x006818),
    (124, "", 0x45684e),
    (125, "darkgreen", 0x004f13),
    (126, "", 0x354f3a),
    (127, "springgreen", 0x00ff7e),
    (128, "", 0xaaffd3),
    (129, "", 0x00bc5e),
    (130, "", 0x7dbc9d),
    (131, "", 0x008140),
    (132, "", 0x55816b),
    (133, "", 0x006834),
    (134, "", 0x456855),
    (135, "", 0x004f27),
    (136, "", 0x354f42),
    (137, "aquamarine", 0x00ffbe),
    (138, "mediumaquamarine", 0xaaffea),
    (139, "", 0x00bc8d),
    (140, "", 0x7dbcac),
    (141, "", 0x00815f),
    (142, "", 0x558176),
    (143, "", 0x00684e),
    (144, "", 0x45685f),
    (145, "", 0x004f3a),
    (146, "darkslate", 0x354f48),
    (147, "", 0x00ffff),
    (148, "", 0xaaffff),
    (149, "turquoise", 0x00bcbc),
    (150, "", 0x7dbcbc),
    (151, "", 0x008181),
    (152, "", 0x558181),
    (153, "", 0x006868),
    (154, "", 0x456868),
    (155, "", 0x004f4f),
    (156, "", 0x354f4f),
    (157, "", 0x00beff),
    (158, "powderblue", 0xaaeaff),
    (159, "", 0x008dbc),
    (160, "steelblue", 0x008dbc),
    (161, "", 0x005f81),
    (162, "", 0x557681),
    (163, "", 0x004e68),
    (164, "", 0x455f68),
    (165, "", 0x003a4f),
    (166, "", 0x35484f),
    (167, "", 0x007eff),
    (168, "lightblue", 0xaad3ff),
    (169, "", 0x005ebc),
    (170, "", 0x7d9dbc),
    (171, "", 0x004081),
    (172, "", 0x556b81),
    (173, "", 0x003468),
    (174, "", 0x455568),
    (175, "", 0x00274f),
    (176, "", 0x35424f),
    (177, "royalblue", 0x003eff),
    (178, "", 0xaabeff),
    (179, "", 0x002dbc),
    (180, "", 0x7d8dbc),
    (181, "", 0x001f81),
    (182, "", 0x555f81),
    (183, "", 0x001868),
    (184, "", 0x454e68),
    (185, "midnight", 0x00134f),
    (186, "", 0x353a4f),
    (187, "", 0x0000ff),
    (188, "", 0xaaaaff),
    (189, "", 0x0000bc),
    (190, "stateblue", 0x7d7dbc),
    (191, "", 0x000081),
    (192, "", 0x555581),
    (193, "navyblue", 0x000068),
    (194, "", 0x454568),
    (195, "", 0x00004f),
    (196, "", 0x35354f),
    (197, "", 0x3e00ff),
    (198, "", 0xbeaaff),
    (199, "", 0x2d00bc),
    (200, "", 0x8d7dbc),
    (201, "", 0x1f0081),
    (202, "", 0x5f5581),
    (203, "", 0x180068),
    (204, "", 0x4e4568),
    (205, "", 0x13004f),
    (206, "", 0x3a354f),
    (207, "blueviolet", 0x7e00ff),
    (208, "", 0xd3aaff),
    (209, "", 0x5e00bc),
    (210, "", 0x9d7dbc),
    (211, "", 0x400081),
    (212, "", 0x6b5581),
    (213, "indigo", 0x340068),
    (214, "", 0x554568),
    (215, "", 0x27004f),
    (216, "", 0x42354f),
    (217, "", 0xbe00ff),
    (218, "", 0xedaaff),
    (219, "darkorchid", 0x8d00bc),
    (220, "", 0xac7dbc),
    (221, "mauve", 0x5f0081),
    (222, "", 0x765581),
    (223, "", 0x4e0068),
    (224, "", 0x5f4568),
    (225, "", 0x3a004f),
    (226, "", 0x48354f),
    (227, "", 0xff00ff),
    (228, "", 0xffaaff),
    (229, "", 0xbc00bc),
    (230, "", 0xbc7dbc),
    (231, "", 0x810081),
    (232, "plum", 0x815581),
    (233, "", 0x680068),
    (234, "", 0x684568),
    (235, "", 0x4f004f),
    (236, "", 0x4f354f),
    (237, "", 0xff00be),
    (238, "violet", 0xffaaea),
    (239, "", 0xbc008d),
    (240, "", 0xbc7dac),
    (241, "", 0x81005f),
    (242, "", 0x815576),
    (243, "maroon", 0x68004e),
    (244, "", 0x68455f),
    (245, "", 0x4f003a),
    (246, "", 0x4f3548),
    (247, "deeppink", 0xff007e),
    (248, "", 0xffaad3),
    (249, "", 0xbc005e),
    (250, "", 0xbc7d9d),
    (251, "", 0x810040),
    (252, "", 0x81556b),
    (253, "", 0x680034),
    (254, "", 0x684555),
    (255, "", 0x4f0027),
    (256, "", 0x4f3542),
    (257, "", 0xff003e),
    (258, "", 0xffaabe),
    (259, "", 0xbc002d),
    (260, "pink", 0xbc7d8d),
    (261, "", 0x81001f),
    (262, "", 0x81555f),
    (263, "", 0x680018),
    (264, "", 0x68454e),
    (265, "", 0x4f0013),
    (266, "", 0x4f353a),
    (267, "", 0x333333),
    (268, "darkgrey", 0x505050),
    (269, "dimgrey", 0x696969),
    (270, "grey", 0x828282),
    (271, "lightgrey", 0xbdbdbd),
    (272, "", 0xffffff),
    (273, "", 0xffffff),
    (274, "", 0xffffff),
    (275, "", 0xffffff),
    (276, "", 0xffffff),
    (277, "", 0xffffff),
    (278, "", 0xffffff),
    (279, "", 0xffffff),
    (280, "", 0xffffff),
    (281, "", 0xffffff),
    (282, "", 0xffffff),
    (283, "", 0xffffff),
    (284, "", 0xffffff),
    (285, "", 0xffffff),
    (286, "", 0xffffff),
    (287, "", 0xffffff),
    (288, "", 0xffffff),
    (289, "", 0xffffff),
    (290, "", 0xffffff),
    (291, "", 0xffffff),
    (292, "", 0xffffff),
    (293, "", 0xffffff),
    (294, "", 0xffffff),
    (295, "", 0xffffff),
    (296, "", 0x5681ad),
    (297, "", 0xffffff),
    (298, "lightyellow", 0xffffaa),
    (299, "white", 0xffffff),
    (300, "midnight", 0x00134f),
    (301, "", 0xffffff),
    (302, "lightyellow", 0xffffaa),
    (303, "gold", 0xffbe00),
    (304, "", 0xffffff),
    (305, "black", 0x000000),
    (306, "white", 0xffffff),
    (307, "whitesmoke", 0xffffff),
    (308, "ivory", 0xffffaa),
    (309, "grey", 0x828282),
    (310, "lightgrey", 0xbdbdbd),
    (311, "darkgrey", 0x505050),
    (312, "darkslate", 0x354f48),
    (313, "red", 0xbc0000),
    (314, "brightred", 0xff0000),
    (315, "coralred", 0xbc2d00),
    (316, "tomato", 0xff3e00),
    (317, "plum", 0x815581),
    (318, "deeppink", 0xff007e),
    (319, "pink", 0xbc7d8d),
    (320, "salmon", 0xffbeaa),
    (321, "orange", 0xffbe00),
    (322, "brightorange", 0xffbe00),
    (323, "orangered", 0xff7e00),
    (324, "maroon", 0x68004e),
    (325, "yellow", 0xffff00),
    (326, "gold", 0xffbe00),
    (327, "lightyellow", 0xffffaa),
    (328, "lightgold", 0xffeaaa),
    (329, "yellowgreen", 0x8dbc00),
    (330, "springgreen", 0x00ff7e),
    (331, "green", 0x00bc00),
    (332, "forestgreen", 0x1f8100),
    (333, "darkgreen", 0x004f13),
    (334, "cyan", 0x00ffff),
    (335, "turquoise", 0x00bcbc),
    (336, "aquamarine", 0x00ffbe),
    (337, "blue", 0x0000ff),
    (338, "royalblue", 0x003eff),
    (339, "navyblue", 0x000068),
    (340, "powderblue", 0xaaeaff),
    (341, "midnight", 0x00134f),
    (342, "steelblue", 0x008dbc),
    (343, "indigo", 0x340068),
    (344, "mauve", 0x5f0081),
    (345, "violet", 0xffaaea),
    (346, "magenta", 0xff00ff),
    (347, "beige", 0xbcac7d),
    (348, "wheat", 0xffd3aa),
    (349, "tan", 0xbc8d7d),
    (350, "sandybrown", 0xbc8d00),
    (351, "brown", 0x683400),
    (352, "khaki", 0x818155),
    (353, "chocolate", 0x4f2700),
    (354, "darkbrown", 0x4f3a00),
    (355, "blueviolet", 0x7e00ff),
    (356, "coral", 0xff7e00),
    (357, "darkorchid", 0x8d00bc),
    (358, "dimgrey", 0x696969),
    (359, "firebrick", 0xbc2d00),
    (360, "indianred", 0xbc7d7d),
    (361, "lightblue", 0xaad3ff),
    (362, "limegreen", 0x3eff00),
    (363, "mediumaquamarine", 0xaaffea),
    (364, "sienna", 0x814000),
    (365, "stateblue", 0x7d7dbc),
];

/// 61 个字典色名 → `0xRRGGBB`（`ColourTableManager` 自己的字典，`COLOUR <name>` 认的就是这些）。
pub const PDMS_COLOUR_DICTIONARY: [(&str, u32); 61] = [
    ("black", 0x000000),
    ("white", 0xffffff),
    ("whitesmoke", 0xffffff),
    ("ivory", 0xffffaa),
    ("grey", 0x828282),
    ("lightgrey", 0xbdbdbd),
    ("darkgrey", 0x505050),
    ("darkslate", 0x354f48),
    ("red", 0xbc0000),
    ("brightred", 0xff0000),
    ("coralred", 0xbc2d00),
    ("tomato", 0xff3e00),
    ("plum", 0x815581),
    ("deeppink", 0xff007e),
    ("pink", 0xbc7d8d),
    ("salmon", 0xffbeaa),
    ("orange", 0xffbe00),
    ("brightorange", 0xffbe00),
    ("orangered", 0xff7e00),
    ("maroon", 0x68004e),
    ("yellow", 0xffff00),
    ("gold", 0xffbe00),
    ("lightyellow", 0xffffaa),
    ("lightgold", 0xffeaaa),
    ("yellowgreen", 0x8dbc00),
    ("springgreen", 0x00ff7e),
    ("green", 0x00bc00),
    ("forestgreen", 0x1f8100),
    ("darkgreen", 0x004f13),
    ("cyan", 0x00ffff),
    ("turquoise", 0x00bcbc),
    ("aquamarine", 0x00ffbe),
    ("blue", 0x0000ff),
    ("royalblue", 0x003eff),
    ("navyblue", 0x000068),
    ("powderblue", 0xaaeaff),
    ("midnight", 0x00134f),
    ("steelblue", 0x008dbc),
    ("indigo", 0x340068),
    ("mauve", 0x5f0081),
    ("violet", 0xffaaea),
    ("magenta", 0xff00ff),
    ("beige", 0xbcac7d),
    ("wheat", 0xffd3aa),
    ("tan", 0xbc8d7d),
    ("sandybrown", 0xbc8d00),
    ("brown", 0x683400),
    ("khaki", 0x818155),
    ("chocolate", 0x4f2700),
    ("darkbrown", 0x4f3a00),
    ("blueviolet", 0x7e00ff),
    ("coral", 0xff7e00),
    ("darkorchid", 0x8d00bc),
    ("dimgrey", 0x696969),
    ("firebrick", 0xbc2d00),
    ("indianred", 0xbc7d7d),
    ("lightblue", 0xaad3ff),
    ("limegreen", 0x3eff00),
    ("mediumaquamarine", 0xaaffea),
    ("sienna", 0x814000),
    ("stateblue", 0x7d7dbc),
];

/// E3D 3.1 出厂图形颜色（`PMLLIB\common\objects\gphcolopt.pmlobj .default()` 与 `gphviewopt.pmlobj .default()`）：
/// 新加元素 / 可见元素 lightgrey，CE yellow，active orange，辅助 aids blue，highlight white，tracing magenta，
/// 背景 grey，剖切盒封口 lightgrey。Design 模块出厂 `autoColour = false`（只有 Spooler 预置两条规则），
/// 所以未配自动着色规则的 E3D 把所有元素都画成 lightgrey #bdbdbd。
pub struct E3dGraphicsColourDefaults {
    pub add_element: &'static str,
    pub visible: &'static str,
    pub ce: &'static str,
    pub active: &'static str,
    pub aids: &'static str,
    pub highlight: &'static str,
    pub tracing: &'static str,
    pub background: &'static str,
    pub clip_box_cap: &'static str,
}

pub const E3D_GRAPHICS_COLOUR_DEFAULTS: E3dGraphicsColourDefaults = E3dGraphicsColourDefaults {
    add_element: "lightgrey",
    visible: "lightgrey",
    ce: "yellow",
    active: "orange",
    aids: "blue",
    highlight: "white",
    tracing: "magenta",
    background: "grey",
    clip_box_cap: "lightgrey",
};

/// 出厂 E3D 3.1 元素默认色 lightgrey = #bdbdbd（真机管子暗边 94 = 189 × Ka 0.5 ✓）。
pub const E3D_DEFAULT_ELEMENT_COLOUR: u32 = 0xbdbdbd;
/// PDMS `grey`（颜色号 1）= #828282，E3D 3.1 出厂背景色（真机截图 130,130,130）。
pub const E3D_BACKGROUND_GREY: u32 = 0x828282;
/// CE（当前元素）高亮 = PDMS `yellow` #ffff00。
pub const E3D_CE_COLOUR: u32 = 0xffff00;
/// 其它选中（highlight）= PDMS `white` #ffffff。
pub const E3D_HIGHLIGHT_COLOUR: u32 = 0xffffff;
/// 辅助对象（aids）= PDMS `blue` #0000ff。
pub const E3D_AIDS_COLOUR: u32 = 0x0000ff;

/// 颜色名归一：去空白、小写、`light grey` / `light-grey` / `light_grey` → `lightgrey`。
pub fn normalize_pdms_colour_name(name: &str) -> String {
    name.trim()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

/// 按颜色号（1..365）查。
pub fn pdms_colour_by_index(index: u16) -> Option<&'static PdmsColourEntry> {
    PDMS_COLOUR_TABLE.iter().find(|entry| entry.0 == index)
}

/// 按颜色名查（大小写 / 空格不敏感）：先查表里的命名槽位（同名多槽取最小颜色号，RGB 一致），
/// 再退到 61 色字典（字典命中时颜色号为 0）。
pub fn pdms_colour_by_name(name: &str) -> Option<PdmsColourEntry> {
    let key = normalize_pdms_colour_name(name);
    if key.is_empty() {
        return None;
    }
    if let Some(entry) = PDMS_COLOUR_TABLE
        .iter()
        .find(|entry| !entry.1.is_empty() && entry.1 == key)
    {
        return Some(*entry);
    }
    PDMS_COLOUR_DICTIONARY
        .iter()
        .find(|(dict_name, _)| *dict_name == key)
        .map(|(dict_name, hex)| (0, *dict_name, *hex))
}

/// 名字或颜色号 → `0xRRGGBB`；查不到返回 `None`。也接受 `pdms:lightgrey` / `pdms:271` 这种带前缀的写法
/// （主题配置里用它与 CSS 颜色名区分）。CSS 名（`gray` / `lightgray`）故意不认。
pub fn pdms_colour_hex(name_or_index: &str) -> Option<u32> {
    let mut s = name_or_index.trim();
    if let Some(rest) = strip_pdms_prefix(s) {
        s = rest.trim();
    }
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
        return s
            .parse::<u16>()
            .ok()
            .and_then(pdms_colour_by_index)
            .map(|entry| entry.2);
    }
    pdms_colour_by_name(s).map(|entry| entry.2)
}

fn strip_pdms_prefix(s: &str) -> Option<&str> {
    let (head, rest) = s.split_at_checked(5)?;
    head.eq_ignore_ascii_case("pdms:").then_some(rest)
}

/// `0xRRGGBB` → `[r, g, b]` sRGB 字节。
pub const fn rgb_bytes(hex: u32) -> [u8; 3] {
    [
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    ]
}

/// `0xRRGGBB` → `#rrggbb`。
pub fn pdms_hex_string(hex: u32) -> String {
    format!("#{hex:06x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_has_every_slot_in_ascending_order() {
        assert_eq!(PDMS_COLOUR_TABLE.len(), 365);
        assert_eq!(PDMS_COLOUR_DICTIONARY.len(), 61);
        for (i, entry) in PDMS_COLOUR_TABLE.iter().enumerate() {
            assert_eq!(entry.0 as usize, i + 1, "颜色号必须从 1 连续递增");
        }
    }

    #[test]
    fn factory_colours_match_the_reverse_engineered_bytes() {
        assert_eq!(pdms_colour_by_index(1).unwrap().2, 0x828282);
        assert_eq!(
            pdms_colour_by_index(271).map(|e| (e.1, e.2)),
            Some(("lightgrey", 0xbdbdbd))
        );
        assert_eq!(
            pdms_colour_by_index(310).map(|e| (e.1, e.2)),
            Some(("lightgrey", 0xbdbdbd))
        );
        assert_eq!(
            pdms_colour_by_index(305).map(|e| (e.1, e.2)),
            Some(("black", 0x000000))
        );
        assert_eq!(
            pdms_colour_hex(E3D_GRAPHICS_COLOUR_DEFAULTS.add_element),
            Some(E3D_DEFAULT_ELEMENT_COLOUR)
        );
        assert_eq!(
            pdms_colour_hex(E3D_GRAPHICS_COLOUR_DEFAULTS.background),
            Some(E3D_BACKGROUND_GREY)
        );
        assert_eq!(
            pdms_colour_hex(E3D_GRAPHICS_COLOUR_DEFAULTS.ce),
            Some(E3D_CE_COLOUR)
        );
        assert_eq!(
            pdms_colour_hex(E3D_GRAPHICS_COLOUR_DEFAULTS.highlight),
            Some(E3D_HIGHLIGHT_COLOUR)
        );
        assert_eq!(
            pdms_colour_hex(E3D_GRAPHICS_COLOUR_DEFAULTS.aids),
            Some(E3D_AIDS_COLOUR)
        );
    }

    #[test]
    fn names_numbers_and_the_pdms_prefix_all_resolve() {
        assert_eq!(pdms_colour_hex("pdms:271"), Some(0xbdbdbd));
        assert_eq!(pdms_colour_hex("PDMS: lightgrey"), Some(0xbdbdbd));
        assert_eq!(pdms_colour_hex("Light Grey"), Some(0xbdbdbd));
        assert_eq!(pdms_colour_hex("pdms:4"), Some(0xffff00));
        assert_eq!(pdms_colour_hex("271"), Some(0xbdbdbd));
        // 同名多槽取最小颜色号：grey 在 1 与 270（与 309）。
        assert_eq!(pdms_colour_by_name("grey").map(|e| e.0), Some(1));
        // 61 个字典色在 305..365 都有命名槽位，所以按名查落到的是表（whitesmoke = 307）；
        // 字典那一路只是表被改坏时的兜底，两边的 RGB 必须一致。
        assert_eq!(
            pdms_colour_by_name("whitesmoke").map(|e| (e.0, e.2)),
            Some((307, 0xffffff))
        );
        for (name, hex) in PDMS_COLOUR_DICTIONARY {
            assert_eq!(pdms_colour_by_name(name).map(|e| e.2), Some(hex), "{name}");
        }
    }

    #[test]
    fn css_names_and_garbage_are_refused() {
        assert_eq!(pdms_colour_hex("gray"), None);
        assert_eq!(pdms_colour_hex("lightgray"), None);
        assert_eq!(pdms_colour_hex(""), None);
        assert_eq!(pdms_colour_hex("pdms:"), None);
        assert_eq!(pdms_colour_hex("0"), None);
        assert_eq!(pdms_colour_hex("366"), None);
        assert_eq!(pdms_colour_hex("99999"), None);
    }

    #[test]
    fn bytes_and_hex_strings_round_trip() {
        assert_eq!(rgb_bytes(0xbdbdbd), [189, 189, 189]);
        assert_eq!(rgb_bytes(0x828282), [130, 130, 130]);
        assert_eq!(pdms_hex_string(0x00bc00), "#00bc00");
    }
}
