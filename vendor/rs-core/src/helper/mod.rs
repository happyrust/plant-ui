use crate::tool::db_tool::db1_dehash;
use crate::tool::float_tool::{f32_round_3, f64_round_3};
use smol_str::SmolStr;
use std::borrow::Cow;
pub mod table;
pub use table::*;

#[inline]
pub fn parse_to_u16(input: &[u8]) -> u16 {
    u16::from_be_bytes(input.try_into().unwrap())
}

#[inline]
pub fn parse_to_i16(input: &[u8]) -> i16 {
    i16::from_be_bytes(input.try_into().unwrap())
}

#[inline]
pub fn parse_to_i32(input: &[u8]) -> i32 {
    i32::from_be_bytes(input.try_into().unwrap())
}

#[inline]
pub fn try_parse_to_i32(input: &[u8]) -> anyhow::Result<i32> {
    Ok(i32::from_be_bytes(input.try_into()?))
}

#[inline]
pub fn parse_to_u32(input: &[u8]) -> u32 {
    u32::from_be_bytes(input.try_into().unwrap())
}

#[inline]
pub fn parse_to_f32(input: &[u8]) -> f32 {
    f32_round_3(f32::from_be_bytes(input.try_into().unwrap()))
}

#[inline]
pub fn parse_to_f64(input: &[u8]) -> f64 {
    return if let [a, b, c, d, e, f, g, h] = input[..8] {
        f64_round_3(f64::from_be_bytes([e, f, g, h, a, b, c, d]))
    } else {
        0.0
    };
}

#[inline]
pub fn convert_u32_to_noun(input: &[u8]) -> SmolStr {
    db1_dehash(parse_to_u32(input.try_into().unwrap())).into()
}

#[inline]
pub fn parse_to_f64_arr(input: &[u8], num: usize) -> Vec<f64> {
    let mut data = vec![];
    for i in 0..num {
        data.push(parse_to_f64(&input[i * 8..i * 8 + 8]));
    }
    data
}

#[inline]
pub fn parse_to_i32_vec(input: &[u8], num: usize) -> Vec<i32> {
    let mut data = vec![];
    for i in 0..num {
        data.push(parse_to_i32(&input[i * 4..i * 4 + 4]));
    }
    data
}

#[inline]
pub fn parse_to_f32_arr(input: &[u8], num: usize) -> Vec<f64> {
    let mut data = vec![];
    for i in 0..num {
        data.push(parse_to_f32(&input[i * 4..i * 4 + 4]) as f64);
    }
    data
}

//去掉不符合条件的 \\u0002 这种，替换成空字符串
#[inline]
pub fn normalize_sql_string(sql: &str) -> String {
    regex::Regex::new(r"\\u[0-9a-fA-F]{4}")
        .unwrap()
        .replace_all(&sql, "")
        .to_string()
}

///添加 “/”
pub fn to_e3d_name(name: &str) -> Cow<'_, str> {
    if name.starts_with('/') {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(format!("/{name}"))
    }
}
