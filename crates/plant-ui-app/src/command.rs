use plant_ui_data::RefU64;
use serde_json::{Value, json};

pub(crate) const QUERY_HELP: &str = "查询命令（REF 可省略，默认当前选中项）：\n\
q identity [REF]       元素身份\n\
q owners [REF]         OWNER 链\n\
q attrs [REF] [FIELDS] 白名单属性，FIELDS 以逗号分隔\n\
q members [REF] [OFFSET] [LIMIT]\n\
q transform [REF]      位姿\n\
q geometry [REF]       几何参数\n\
q catalog [REF]        元件库引用\n\
q room [REF]           房间归属\n\
q root [REF]           最小生成根\n\
q impact <ATTRIBUTE>   变更影响\n\
q pending [DBNUM] [OFFSET] [LIMIT] [--live]\n\
q bounds [REF]         世界包围盒\n\
完整 MCP 工具名可替换短命令；q prop <属性> 强制查询本地属性。";

#[derive(Debug, PartialEq)]
pub(crate) enum ParsedCommand {
    Empty,
    Help,
    Clear,
    /// 强制重建子串搜索索引。
    ///
    /// 它是「戳看不见的改动」唯一的门：没有 gen-model 水位的设计库里发生纯改名，
    /// 行数与水位都不动，自动校验永远发现不了（ADR-0023 决定 5）。
    Reindex,
    Query(QueryInput),
    LocateName(String),
    LocateRef(RefU64),
    Error(String),
}

#[derive(Debug, PartialEq)]
pub(crate) enum QueryInput {
    Help,
    Property(String),
    Remote(RemoteQuery),
}

#[derive(Debug, PartialEq)]
pub(crate) struct RemoteQuery {
    pub alias: &'static str,
    pub tool: &'static str,
    args: RemoteArgs,
}

#[derive(Debug, PartialEq)]
enum RemoteArgs {
    Target(Option<RefU64>),
    Attributes {
        target: Option<RefU64>,
        fields: Vec<String>,
    },
    Members {
        target: Option<RefU64>,
        offset: usize,
        limit: usize,
    },
    Impact(String),
    Pending {
        dbnum: Option<u32>,
        offset: usize,
        limit: usize,
        include_dead: bool,
    },
}

pub(crate) struct BoundQuery {
    pub label: String,
    pub tool: &'static str,
    pub arguments: Value,
}

impl RemoteQuery {
    pub fn bind(&self, selected: Option<RefU64>) -> Result<BoundQuery, String> {
        let (label, arguments) = match &self.args {
            RemoteArgs::Target(explicit) => {
                let refno = target(*explicit, selected)?;
                let pdms = refno.to_pdms_str();
                (format!("{} {pdms}", self.alias), json!({ "refno": pdms }))
            }
            RemoteArgs::Attributes {
                target: explicit,
                fields,
            } => {
                let refno = target(*explicit, selected)?;
                let pdms = refno.to_pdms_str();
                (
                    format!("{} {pdms}", self.alias),
                    json!({ "refno": pdms, "fields": fields }),
                )
            }
            RemoteArgs::Members {
                target: explicit,
                offset,
                limit,
            } => {
                let refno = target(*explicit, selected)?;
                let pdms = refno.to_pdms_str();
                (
                    format!("{} {pdms}", self.alias),
                    json!({ "refno": pdms, "offset": offset, "limit": limit }),
                )
            }
            RemoteArgs::Impact(attribute) => (
                format!("{} {}", self.alias, attribute.to_ascii_uppercase()),
                json!({ "attribute": attribute }),
            ),
            RemoteArgs::Pending {
                dbnum,
                offset,
                limit,
                include_dead,
            } => (
                format!(
                    "{} db={}",
                    self.alias,
                    dbnum.map_or("*".into(), |v| v.to_string())
                ),
                json!({
                    "dbnum": dbnum,
                    "offset": offset,
                    "limit": limit,
                    "include_dead": include_dead,
                }),
            ),
        };
        Ok(BoundQuery {
            label,
            tool: self.tool,
            arguments,
        })
    }
}

fn target(explicit: Option<RefU64>, selected: Option<RefU64>) -> Result<RefU64, String> {
    explicit
        .or(selected)
        .ok_or_else(|| "当前未选中元素；请先选择元素或显式提供 REF".into())
}

pub(crate) fn parse(input: &str) -> ParsedCommand {
    let input = input.trim();
    if input.is_empty() {
        return ParsedCommand::Empty;
    }
    if input.eq_ignore_ascii_case("help") {
        return ParsedCommand::Help;
    }
    if input.eq_ignore_ascii_case("clear") {
        return ParsedCommand::Clear;
    }
    if input.eq_ignore_ascii_case("reindex") {
        return ParsedCommand::Reindex;
    }
    if let Some(name) = input.strip_prefix('/') {
        let name = name.trim();
        return if name.is_empty() {
            ParsedCommand::Error("用法：/<名称>".into())
        } else {
            ParsedCommand::LocateName(name.into())
        };
    }
    if let Some(refno) = input.strip_prefix('=') {
        let refno = refno.trim();
        return if refno.is_empty() {
            ParsedCommand::Error("用法：=<参考号>".into())
        } else {
            refno.parse().map_or_else(
                |_| ParsedCommand::Error(format!("无效参考号：{refno}")),
                ParsedCommand::LocateRef,
            )
        };
    }

    let mut words = input.splitn(2, char::is_whitespace);
    let command = words.next().unwrap_or_default();
    let arg = words.next().unwrap_or_default().trim();
    if command.eq_ignore_ascii_case("q") {
        return parse_query(arg);
    }
    ParsedCommand::Error(format!("未知命令：{command}；输入 help 查看可用命令"))
}

fn parse_query(input: &str) -> ParsedCommand {
    if input.is_empty() {
        return ParsedCommand::Error("用法：q <属性|查询命令>；输入 q help 查看查询帮助".into());
    }
    let words = input.split_whitespace().collect::<Vec<_>>();
    let name = words[0].to_ascii_lowercase();
    let rest = &words[1..];
    if name == "help" {
        return if rest.is_empty() {
            ParsedCommand::Query(QueryInput::Help)
        } else {
            ParsedCommand::Error("用法：q help".into())
        };
    }
    if name == "prop" {
        return if rest.len() == 1 {
            ParsedCommand::Query(QueryInput::Property(rest[0].into()))
        } else {
            ParsedCommand::Error("用法：q prop <属性>".into())
        };
    }

    let Some((alias, tool)) = query_tool(&name) else {
        return if rest.is_empty() {
            ParsedCommand::Query(QueryInput::Property(words[0].into()))
        } else {
            ParsedCommand::Error(format!(
                "未知查询命令：{}；输入 q help 查看可用查询",
                words[0]
            ))
        };
    };
    match parse_remote(alias, tool, rest) {
        Ok(query) => ParsedCommand::Query(QueryInput::Remote(query)),
        Err(error) => ParsedCommand::Error(error),
    }
}

fn query_tool(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "identity" | "e3d.element.identity" => ("identity", "e3d.element.identity"),
        "owners" | "e3d.element.owner_chain" => ("owners", "e3d.element.owner_chain"),
        "attrs" | "e3d.element.attributes" => ("attrs", "e3d.element.attributes"),
        "members" | "e3d.element.members" => ("members", "e3d.element.members"),
        "transform" | "e3d.element.transform" => ("transform", "e3d.element.transform"),
        "geometry" | "e3d.geometry.parameters" => ("geometry", "e3d.geometry.parameters"),
        "catalog" | "e3d.catalog.references" => ("catalog", "e3d.catalog.references"),
        "room" | "e3d.room.lookup" => ("room", "e3d.room.lookup"),
        "root" | "model.generation_root" => ("root", "model.generation_root"),
        "impact" | "model.change_impact" => ("impact", "model.change_impact"),
        "pending" | "model.pending_units" => ("pending", "model.pending_units"),
        "bounds" | "model.spatial.bounds" => ("bounds", "model.spatial.bounds"),
        _ => return None,
    })
}

fn parse_remote(
    alias: &'static str,
    tool: &'static str,
    words: &[&str],
) -> Result<RemoteQuery, String> {
    let args = match tool {
        "e3d.element.attributes" => parse_attributes(words)?,
        "e3d.element.members" => parse_members(words)?,
        "model.change_impact" => {
            if words.len() != 1 {
                return Err("用法：q impact <ATTRIBUTE>".into());
            }
            RemoteArgs::Impact(words[0].into())
        }
        "model.pending_units" => parse_pending(words)?,
        _ => {
            if words.len() > 1 {
                return Err(format!("用法：q {alias} [REF]"));
            }
            let explicit = words.first().map(|word| parse_ref(word)).transpose()?;
            RemoteArgs::Target(explicit)
        }
    };
    Ok(RemoteQuery { alias, tool, args })
}

fn parse_attributes(words: &[&str]) -> Result<RemoteArgs, String> {
    let (target, fields) = match words.first() {
        Some(word) if looks_like_ref(word) => (Some(parse_ref(word)?), &words[1..]),
        _ => (None, words),
    };
    let fields = fields
        .iter()
        .flat_map(|word| word.split(','))
        .filter(|field| !field.is_empty())
        .map(|field| field.to_ascii_lowercase())
        .collect::<Vec<_>>();
    for field in &fields {
        if !matches!(
            field.as_str(),
            "name" | "type" | "noun" | "owner" | "position"
        ) {
            return Err(format!(
                "未知属性字段：{field}；可用 name,type,owner,position"
            ));
        }
    }
    Ok(RemoteArgs::Attributes { target, fields })
}

fn parse_members(words: &[&str]) -> Result<RemoteArgs, String> {
    let (target, numbers) = match words.first() {
        Some(word) if looks_like_ref(word) => (Some(parse_ref(word)?), &words[1..]),
        _ => (None, words),
    };
    if numbers.len() > 2 {
        return Err("用法：q members [REF] [OFFSET] [LIMIT]".into());
    }
    let offset = parse_usize(numbers.first().copied(), 0, "OFFSET")?;
    let limit = parse_usize(numbers.get(1).copied(), 200, "LIMIT")?;
    validate_limit(limit)?;
    Ok(RemoteArgs::Members {
        target,
        offset,
        limit,
    })
}

fn parse_pending(words: &[&str]) -> Result<RemoteArgs, String> {
    let mut numbers = Vec::new();
    let mut include_dead = true;
    for word in words {
        if word.eq_ignore_ascii_case("--live") {
            include_dead = false;
        } else {
            numbers.push(*word);
        }
    }
    if numbers.len() > 3 {
        return Err("用法：q pending [DBNUM] [OFFSET] [LIMIT] [--live]".into());
    }
    let dbnum = numbers
        .first()
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|_| format!("无效 DBNUM：{value}"))
        })
        .transpose()?;
    let offset = parse_usize(numbers.get(1).copied(), 0, "OFFSET")?;
    let limit = parse_usize(numbers.get(2).copied(), 200, "LIMIT")?;
    validate_limit(limit)?;
    Ok(RemoteArgs::Pending {
        dbnum,
        offset,
        limit,
        include_dead,
    })
}

fn parse_ref(value: &str) -> Result<RefU64, String> {
    value.parse().map_err(|_| format!("无效参考号：{value}"))
}

fn looks_like_ref(value: &str) -> bool {
    value.contains(['/', '_', '=', ':'])
        || value
            .parse::<u64>()
            .is_ok_and(|packed| packed > u32::MAX as u64)
}

fn parse_usize(value: Option<&str>, default: usize, label: &str) -> Result<usize, String> {
    value.map_or(Ok(default), |value| {
        value.parse().map_err(|_| format!("无效 {label}：{value}"))
    })
}

fn validate_limit(limit: usize) -> Result<(), String> {
    if (1..=1000).contains(&limit) {
        Ok(())
    } else {
        Err("LIMIT 必须在 1..=1000".into())
    }
}

pub(crate) fn format_query_result(tool: &str, value: &Value) -> String {
    match tool {
        "e3d.element.identity" => scalar_lines(value, &["refno", "ce", "noun", "name"]),
        "e3d.element.owner_chain" => {
            let mut lines = vec![format!("REFNO = {}", text(value, "refno"))];
            for (index, node) in array(value, "nodes").iter().enumerate() {
                lines.push(format!(
                    "{index:02}  {}  {}",
                    text(node, "noun"),
                    text(node, "name")
                ));
            }
            lines.push(format!(
                "完整 = {}；截断 = {}",
                text(value, "complete"),
                text(value, "truncated")
            ));
            lines.join("\n")
        }
        "e3d.element.attributes" => scalar_lines(
            value,
            &[
                "refno",
                "name",
                "noun",
                "owner",
                "position_mm",
                "unsupported_fields",
            ],
        ),
        "e3d.element.members" => {
            let mut lines = page_header(value, "成员");
            for item in array(value, "items") {
                lines.push(format!(
                    "{}  {}  {}  {}",
                    text(item, "index"),
                    text(item, "noun"),
                    text(item, "value"),
                    text(item, "refno")
                ));
            }
            lines.join("\n")
        }
        "e3d.element.transform" => scalar_lines(
            value,
            &["refno", "position_mm", "orientation", "unsupported_fields"],
        ),
        "e3d.geometry.parameters" => {
            scalar_lines(value, &["refno", "noun", "values", "unsupported_fields"])
        }
        "e3d.catalog.references" => {
            let mut lines = vec![format!("REFNO = {}", text(value, "refno"))];
            for item in array(value, "references") {
                lines.push(format!(
                    "{} = {}",
                    text(item, "kind").to_ascii_uppercase(),
                    text(item, "value")
                ));
            }
            append_unsupported(&mut lines, value);
            lines.join("\n")
        }
        "e3d.room.lookup" => {
            let memberships = array(value, "memberships");
            let mut lines = vec![format!("房间归属：{} 项", memberships.len())];
            for item in memberships {
                lines.push(format!(
                    "{}  room={}  panel={}",
                    text(item, "room_num"),
                    text(item, "room_refno"),
                    text(item, "panel_refno")
                ));
            }
            lines.join("\n")
        }
        "model.generation_root" => scalar_lines(value, &["refno", "root", "noun", "name", "kind"]),
        "model.change_impact" => {
            scalar_lines(value, &["attribute", "effect", "affects_model", "action"])
        }
        "model.pending_units" => {
            let mut lines = page_header(value, "待重试单元");
            lines.push(format!("死信 = {}", text(value, "dead_count")));
            for item in array(value, "units") {
                lines.push(format!(
                    "db={}  {} {}  attempts={}  dead={}  error={}",
                    text(item, "dbnum"),
                    text(item, "noun"),
                    text(item, "root_refno"),
                    text(item, "attempts"),
                    text(item, "dead"),
                    text(item, "last_error")
                ));
            }
            lines.join("\n")
        }
        "model.spatial.bounds" => scalar_lines(value, &["refno", "min_mm", "max_mm", "source"]),
        _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn scalar_lines(value: &Value, fields: &[&str]) -> String {
    fields
        .iter()
        .filter_map(|field| {
            value.get(*field).and_then(|value| {
                (!value.is_null())
                    .then(|| format!("{} = {}", field.to_ascii_uppercase(), display(value)))
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn page_header(value: &Value, title: &str) -> Vec<String> {
    let offset = value.get("offset").and_then(Value::as_u64).unwrap_or(0);
    let count = value
        .get("items")
        .or_else(|| value.get("units"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let total = value.get("total").and_then(Value::as_u64).unwrap_or(0);
    let mut lines = vec![format!(
        "{title}：显示 {offset}..{} / {total}",
        offset + count as u64
    )];
    if value.get("truncated").and_then(Value::as_bool) == Some(true) {
        lines.push(format!(
            "结果已截断；下一页 OFFSET = {}",
            offset + count as u64
        ));
    }
    lines
}

fn append_unsupported(lines: &mut Vec<String>, value: &Value) {
    let unsupported = array(value, "unsupported_fields");
    if !unsupported.is_empty() {
        lines.push(format!(
            "不支持 = {}",
            unsupported
                .iter()
                .map(display)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}

fn array<'a>(value: &'a Value, field: &str) -> &'a [Value] {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn text(value: &Value, field: &str) -> String {
    value.get(field).map(display).unwrap_or_else(|| "-".into())
}

fn display(value: &Value) -> String {
    match value {
        Value::Null => "-".into(),
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aliases_full_names_and_legacy_properties() {
        assert!(matches!(
            parse("q help"),
            ParsedCommand::Query(QueryInput::Help)
        ));
        assert!(matches!(
            parse("q NAME"),
            ParsedCommand::Query(QueryInput::Property(name)) if name == "NAME"
        ));
        assert!(matches!(
            parse("q identity"),
            ParsedCommand::Query(QueryInput::Remote(RemoteQuery {
                tool: "e3d.element.identity",
                ..
            }))
        ));
        assert!(matches!(
            parse("q model.change_impact POS"),
            ParsedCommand::Query(QueryInput::Remote(RemoteQuery {
                tool: "model.change_impact",
                ..
            }))
        ));
    }

    #[test]
    fn binds_selected_and_explicit_targets_to_pdms_refnos() {
        let selected = "24381/100819".parse().unwrap();
        let ParsedCommand::Query(QueryInput::Remote(query)) = parse("q members 5 10") else {
            panic!("remote query expected")
        };
        let bound = query.bind(Some(selected)).unwrap();
        assert_eq!(bound.arguments["refno"], "24381/100819");
        assert_eq!(bound.arguments["offset"], 5);
        assert_eq!(bound.arguments["limit"], 10);

        let ParsedCommand::Query(QueryInput::Remote(query)) =
            parse("q e3d.element.identity 24381_100600")
        else {
            panic!("remote query expected")
        };
        assert_eq!(query.bind(None).unwrap().arguments["refno"], "24381/100600");

        let packed = selected.0.to_string();
        let ParsedCommand::Query(QueryInput::Remote(query)) =
            parse(&format!("q attrs {packed} name"))
        else {
            panic!("packed RefU64 target expected")
        };
        assert_eq!(query.bind(None).unwrap().arguments["refno"], "24381/100819");
    }

    #[test]
    fn validates_fields_limits_and_pending_live_filter() {
        assert!(matches!(parse("q attrs colour"), ParsedCommand::Error(_)));
        assert!(matches!(parse("q members 0 1001"), ParsedCommand::Error(_)));
        let ParsedCommand::Query(QueryInput::Remote(query)) = parse("q pending 24381 20 50 --live")
        else {
            panic!("remote query expected")
        };
        let bound = query.bind(None).unwrap();
        assert_eq!(bound.arguments["dbnum"], 24381);
        assert_eq!(bound.arguments["include_dead"], false);
    }

    #[test]
    fn formatter_hides_raw_output_and_marks_truncation() {
        let output = format_query_result(
            "e3d.element.members",
            &json!({
                "offset": 0,
                "limit": 1,
                "total": 2,
                "truncated": true,
                "items": [{"index": 1, "noun": "DAMP", "value": "/D1", "refno": "24381/1"}],
                "raw_output": "secret transcript"
            }),
        );
        assert!(output.contains("结果已截断"));
        assert!(output.contains("1  DAMP  /D1  24381/1"));
        assert!(!output.contains("secret transcript"));
    }

    #[test]
    fn fixed_tool_formatters_match_snapshots() {
        let cases = [
            (
                "e3d.element.identity",
                json!({"refno":"1/2","ce":"/X","noun":"EQUI","name":"/X"}),
                "REFNO = 1/2\nCE = /X\nNOUN = EQUI\nNAME = /X",
            ),
            (
                "e3d.element.owner_chain",
                json!({"refno":"1/2","nodes":[{"noun":"EQUI","name":"/X"}],"complete":true,"truncated":false}),
                "REFNO = 1/2\n00  EQUI  /X\n完整 = true；截断 = false",
            ),
            (
                "e3d.element.attributes",
                json!({"refno":"1/2","name":"/X","noun":"EQUI","owner":"1/1","position_mm":[1,2,3],"unsupported_fields":[]}),
                "REFNO = 1/2\nNAME = /X\nNOUN = EQUI\nOWNER = 1/1\nPOSITION_MM = [1,2,3]\nUNSUPPORTED_FIELDS = []",
            ),
            (
                "e3d.element.members",
                json!({"offset":0,"total":0,"truncated":false,"items":[]}),
                "成员：显示 0..0 / 0",
            ),
            (
                "e3d.element.transform",
                json!({"refno":"1/2","position_mm":[1,2,3],"orientation":null,"unsupported_fields":["orientation"]}),
                "REFNO = 1/2\nPOSITION_MM = [1,2,3]\nUNSUPPORTED_FIELDS = [\"orientation\"]",
            ),
            (
                "e3d.geometry.parameters",
                json!({"refno":"1/2","noun":"CYLI","values":{"diameter":"2"},"unsupported_fields":[]}),
                "REFNO = 1/2\nNOUN = CYLI\nVALUES = {\"diameter\":\"2\"}\nUNSUPPORTED_FIELDS = []",
            ),
            (
                "e3d.catalog.references",
                json!({"refno":"1/2","references":[{"kind":"spec","value":"3/4"}],"unsupported_fields":["catalog"]}),
                "REFNO = 1/2\nSPEC = 3/4\n不支持 = catalog",
            ),
            (
                "e3d.room.lookup",
                json!({"memberships":[]}),
                "房间归属：0 项",
            ),
            (
                "model.generation_root",
                json!({"refno":"1/2","root":"1/2","noun":"EQUI","name":"/X","kind":"delivery_unit"}),
                "REFNO = 1/2\nROOT = 1/2\nNOUN = EQUI\nNAME = /X\nKIND = delivery_unit",
            ),
            (
                "model.change_impact",
                json!({"attribute":"POS","effect":"transform_only","affects_model":true,"action":"transform"}),
                "ATTRIBUTE = POS\nEFFECT = transform_only\nAFFECTS_MODEL = true\nACTION = transform",
            ),
            (
                "model.pending_units",
                json!({"offset":0,"total":0,"truncated":false,"dead_count":0,"units":[]}),
                "待重试单元：显示 0..0 / 0\n死信 = 0",
            ),
            (
                "model.spatial.bounds",
                json!({"refno":"1/2","min_mm":[0,0,0],"max_mm":[1,1,1],"source":"inst_relate"}),
                "REFNO = 1/2\nMIN_MM = [0,0,0]\nMAX_MM = [1,1,1]\nSOURCE = inst_relate",
            ),
        ];
        for (tool, value, expected) in cases {
            assert_eq!(format_query_result(tool, &value), expected, "{tool}");
        }
    }
}
