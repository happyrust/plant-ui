//! 设计令牌与组件层（迁自 rs-plant ui/redesign 分支，ADR-0004 的单一源延续）。

pub mod theme_tokens;
pub mod tokens;
pub mod widgets;

pub(crate) fn group_number(value: i64) -> String {
    let negative = value < 0;
    let digits = value.abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(ch);
    }
    if negative { format!("-{out}") } else { out }
}
