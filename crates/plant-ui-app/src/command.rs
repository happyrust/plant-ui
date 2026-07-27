use plant_ui_data::RefU64;

#[derive(Debug, PartialEq)]
pub(crate) enum ParsedCommand {
    Empty,
    Help,
    Clear,
    Query(String),
    LocateName(String),
    LocateRef(RefU64),
    Error(String),
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
        return if arg.is_empty() {
            ParsedCommand::Error("用法：q <属性>".into())
        } else {
            ParsedCommand::Query(arg.into())
        };
    }

    ParsedCommand::Error(format!("未知命令：{command}；输入 help 查看可用命令"))
}

#[cfg(test)]
mod tests {
    use super::{ParsedCommand, parse};

    #[test]
    fn parses_supported_commands_and_reports_invalid_input() {
        let cases = [
            ("", ParsedCommand::Empty),
            ("  HELP  ", ParsedCommand::Help),
            ("clear", ParsedCommand::Clear),
            ("Q name", ParsedCommand::Query("name".into())),
            ("/VESSEL-01", ParsedCommand::LocateName("VESSEL-01".into())),
            ("q", ParsedCommand::Error("用法：q <属性>".into())),
            ("/", ParsedCommand::Error("用法：/<名称>".into())),
            ("=", ParsedCommand::Error("用法：=<参考号>".into())),
            (
                "=not-a-refno",
                ParsedCommand::Error("无效参考号：not-a-refno".into()),
            ),
            (
                "exit",
                ParsedCommand::Error("未知命令：exit；输入 help 查看可用命令".into()),
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(parse(input), expected, "input: {input:?}");
        }
    }
}
