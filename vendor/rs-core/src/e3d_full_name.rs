//! E3D 未命名元素的默认全名（伪属性 `FLNM`），复刻自 `core.dll`。
//!
//! 出处是 `dbgenlib/ELNAME` 与 `dbgenlib/LNPART` 两个函数（E3D 3.1 `core.dll`，
//! IDA 反编译）。`ELNAME` 按请求的伪属性分支，`FLNM`（hash 797856）这一支是：
//!
//! ```text
//! 元素自己有名字  → 就是 NAME，不带名词      /C-IY-1R330-B
//! 元素自己没名字  → LNPART 一转写一段，BRETUR 换成 OWNER 再来一转，
//!                   直到某一转落在有名字的祖先上
//! ```
//!
//! `LNPART` 每转写的那一段是：
//!
//! ```text
//! <名词全词> + " " + ( 这一级有名字 ? <名字>        —— 收尾
//!                                  : <序号> + " of " —— 继续走 OWNER )
//! ```
//!
//! 于是嵌套形态自然长成 `SNOUT 1 of TMPLATE 1 of EQUIPMENT /03SKID3-EQUIP1`：
//! 元素自己有名字时不带名词，但收尾的那个**祖先**带名词（`LNPART` 每一转都先写名词）。
//!
//! **链不上到库根**：WORLD 不进名字，无名 SITE 就叫 `SITE 10`，不是
//! `SITE 10 of WORLD /*XXX`（2026-09-15 按 E3D 实际显示定的口径）。
//!
//! 从二进制里读出来、写代码时最容易做错的几处：
//!
//! 1. **名词是全词**。`LNPART` 走 `ATNTXT(noun, NAME)`，而 `DB_Noun::getField(NAME)`
//!    与 `DB_Noun::fullName()` 是同一段代码，给的是 `BRANCH` / `EQUIPMENT` / `TMPLATE`，
//!    不是库里存的 `BRAN` / `EQUI` / `TMPL`。全词从 [`noun_full_name`] 查。
//! 2. **分隔符是小写 ` of `**。`LNPART` 一个字符一个字符写：`0x20, 'o', 'f', 0x20`。
//! 3. **序号是同名词兄弟里的第几个**（`ITYPOS` 沿成员链只数 `actualType` 相同的），
//!    从 1 起，不是「在属主全部成员里的下标」，也不是「第几个无名兄弟」。
//! 4. **收尾那一截带名词，元素自己那一截不带**（见上）。
//! 5. **名字不以 `/` 开头时 `LNPART` 自己补一个**。
//!
//! 服务端（gen-model `data_interface::e3d_full_name`）与 SurrealQL
//! （`fn::default_full_name`）各有一份同规则的实现，三处必须给出一字不差的名字：
//! plant-ui 的双面对拍探针（`read_face::parity`）是逐节点比 `name` 的。三份共用
//! 同一张 `noun_full_names.json`，由 gen-model `scripts/gen_noun_full_names.py` 生成。

use std::collections::HashMap;
use std::sync::OnceLock;

/// `resource/dict/noun_full_names.json`：短名词 → 名词全词，只收两者不同的那些。
///
/// 查不到就退回短名词——1418 个名词的全词本来就等于短名词，表里不收它们。
const NOUN_FULL_NAMES_JSON: &str = include_str!("../resource/dict/noun_full_names.json");

fn noun_full_names() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(NOUN_FULL_NAMES_JSON).unwrap_or_default())
}

/// 短名词的名词全词：`BRAN` → `BRANCH`，`SITE` → `SITE`，未登记的原样返回。
pub fn noun_full_name(noun: &str) -> &str {
    let noun = noun.trim();
    noun_full_names()
        .get(noun)
        .map(String::as_str)
        .unwrap_or(noun)
}

/// `FLNM` 名字链上的一级：元素自己、它的 OWNER、OWNER 的 OWNER……
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NameLink {
    /// 库里存的短名词（`pe.noun`）。拼名字时由 [`noun_full_name`] 换成全词。
    pub noun: String,
    /// 存着的 `name`，去掉首尾空白；无名元素是空串。
    pub name: String,
    /// 在属主的**同名词**成员里排第几（`ITYPOS`，从 1 起）。有名字的那一级用不上。
    pub ordinal: usize,
    /// 这一级是库根（WORLD，没有 owner）。库根不进名字：E3D 里无名 SITE 就叫
    /// `SITE 10`，不是 `SITE 10 of WORLD /*XXX`，链碰到它就收手。
    pub is_root: bool,
}

impl NameLink {
    pub fn named(noun: impl Into<String>, name: impl Into<String>) -> Self {
        Self { noun: noun.into(), name: name.into(), ordinal: 0, is_root: false }
    }

    pub fn unnamed(noun: impl Into<String>, ordinal: usize) -> Self {
        Self { noun: noun.into(), name: String::new(), ordinal, is_root: false }
    }

    /// 库根（WORLD）：只用来让链在这里收手，本身不写进名字。
    pub fn root() -> Self {
        Self { is_root: true, ..Default::default() }
    }

    fn is_named(&self) -> bool {
        !self.name.is_empty()
    }
}

/// `FLNM`：`chain[0]` 是元素自己，其后是 OWNER 链，直到第一个有名字的祖先。
///
/// 链走到头都没有名字（顶到 WORLD 还是无名，或者调用方截短了链）时，最后一段的
/// ` of ` 尾巴会被去掉——E3D 到不了这一步（WORLD 总有名字），这里只是不让残缺的
/// 链拼出一个以 ` of ` 结尾的名字。
pub fn full_name(chain: &[NameLink]) -> String {
    let Some(element) = chain.first() else {
        return String::new();
    };
    // 元素自己有名字：`ELNAME` 在进 `LNPART` 之前就收工了，只给名字、不带名词。
    if element.is_named() {
        return element.name.clone();
    }
    owner_tail(chain)
}

/// 无名元素的默认全名，链的尾巴已经拼好时用（同一批兄弟共用一条尾巴）。
///
/// `tail` 就是 [`full_name`] 对属主那条链的结果里、`<名词全词> ` 之后的部分，
/// 即 `owner_tail(&owner_chain)`。
pub fn with_tail(noun: &str, ordinal: usize, tail: &str) -> String {
    if tail.is_empty() {
        return format!("{} {ordinal}", noun_full_name(noun));
    }
    format!("{} {ordinal} of {tail}", noun_full_name(noun))
}

/// 一条链走完 `LNPART`：每一级都先写名词全词，落在有名字的那级收尾。
///
/// 这是 `LNPART` 本身，[`full_name`] 只是在它前面加了「元素自己有名字就不进来」
/// 这一道。喂属主链进来就得到同一批兄弟共用的尾巴：`BRANCH /C-IY-1R330-B`、
/// `TMPLATE 1 of EQUIPMENT /03SKID3-EQUIP1`。
pub fn owner_tail(chain: &[NameLink]) -> String {
    let mut out = String::new();
    for link in chain {
        if link.is_root {
            break;
        }
        out.push_str(noun_full_name(&link.noun));
        out.push(' ');
        if link.is_named() {
            out.push_str(&slashed(&link.name));
            return out;
        }
        out.push_str(&link.ordinal.to_string());
        out.push_str(" of ");
    }
    out.truncate(out.trim_end_matches(" of ").len());
    out
}

/// `LNPART` 写名字前会看第一个字符：不是 `/` 就补一个。
fn slashed(name: &str) -> String {
    if name.starts_with('/') {
        name.to_string()
    } else {
        format!("/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表里收的是两者不同的那些；相同的（SITE/ZONE/PIPE…）查不到，退回短名词即可。
    #[test]
    fn short_nouns_expand_to_the_words_e3d_prints() {
        assert_eq!(noun_full_name("BRAN"), "BRANCH");
        assert_eq!(noun_full_name("EQUI"), "EQUIPMENT");
        assert_eq!(noun_full_name("TMPL"), "TMPLATE");
        assert_eq!(noun_full_name("WORL"), "WORLD");
        assert_eq!(noun_full_name("SUPPO"), "SUPPORT");
        assert_eq!(noun_full_name("SITE"), "SITE");
        assert_eq!(noun_full_name("SNOU"), "SNOUT");
        assert_eq!(noun_full_name(":SDS_Pole"), ":SDS_Pole");
    }

    /// 有名字的元素只给名字——名词是 `LNPART` 写的，而 `ELNAME` 这一支根本不进 `LNPART`。
    #[test]
    fn a_named_element_is_just_its_name() {
        assert_eq!(
            full_name(&[NameLink::named("BRAN", "/C-IY-1R330-B")]),
            "/C-IY-1R330-B"
        );
    }

    /// `core.dll` 的两个实测形态：一层与嵌套两层，收尾的祖先都带名词。
    #[test]
    fn unnamed_elements_chain_up_to_the_first_named_ancestor() {
        let simple = [
            NameLink::unnamed("BEND", 3),
            NameLink::named("BRAN", "/C-IY-1R330-B"),
        ];
        assert_eq!(full_name(&simple), "BEND 3 of BRANCH /C-IY-1R330-B");

        let nested = [
            NameLink::unnamed("SNOU", 1),
            NameLink::unnamed("TMPL", 1),
            NameLink::named("EQUI", "/03SKID3-EQUIP1"),
        ];
        assert_eq!(
            full_name(&nested),
            "SNOUT 1 of TMPLATE 1 of EQUIPMENT /03SKID3-EQUIP1"
        );

    }

    /// 库根不进名字：截图里那条无名 SITE 就叫 `SITE 10`，后面不跟 ` of WORLD /*XXX`。
    #[test]
    fn the_chain_stops_before_the_database_root() {
        let site = [NameLink::unnamed("SITE", 10), NameLink::root()];
        assert_eq!(full_name(&site), "SITE 10");
        assert_eq!(owner_tail(&[NameLink::root()]), "");
    }

    /// 分隔符是小写 ` of `；旧实现写的是大写 ` OF `。
    #[test]
    fn the_separator_is_lowercase() {
        let chain = [NameLink::unnamed("ZONE", 1), NameLink::named("SITE", "/S1")];
        assert!(full_name(&chain).contains(" of "));
        assert!(!full_name(&chain).contains(" OF "));
    }

    /// 名字不以 `/` 开头时 `LNPART` 补一个。
    #[test]
    fn a_name_without_a_leading_slash_gets_one() {
        let chain = [NameLink::unnamed("BEND", 2), NameLink::named("BRAN", "B1")];
        assert_eq!(full_name(&chain), "BEND 2 of BRANCH /B1");
    }

    /// 尾巴单独拼出来，再跟每个兄弟的名词与序号拼一次，结果与整条链一次拼完相同。
    #[test]
    fn a_shared_tail_rebuilds_the_same_name() {
        let owner_chain = [
            NameLink::unnamed("TMPL", 1),
            NameLink::named("EQUI", "/03SKID3-EQUIP1"),
        ];
        let tail = owner_tail(&owner_chain);
        assert_eq!(tail, "TMPLATE 1 of EQUIPMENT /03SKID3-EQUIP1");

        let mut whole = vec![NameLink::unnamed("SNOU", 1)];
        whole.extend(owner_chain.iter().cloned());
        assert_eq!(with_tail("SNOU", 1, &tail), full_name(&whole));
    }

    /// 链走到头还没碰到有名字的祖先：不留 ` of ` 尾巴。
    #[test]
    fn a_chain_with_no_named_ancestor_does_not_end_in_of() {
        let chain = [NameLink::unnamed("ZONE", 4), NameLink::unnamed("SITE", 1)];
        assert_eq!(full_name(&chain), "ZONE 4 of SITE 1");
        assert_eq!(with_tail("SITE", 4, ""), "SITE 4");
        assert!(full_name(&[]).is_empty());
    }
}
