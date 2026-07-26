//! 运行中应用的验收探针：走 egui_inspection 线协议（M0-5 起监听的 5719 端口），
//! 读控件树、注入输入、截图。
//!
//! 比 PrintWindow + SetCursorPos 那条路好在三点：不抢窗口焦点（不会误触到别的程序）、
//! 坐标就是 egui 逻辑点（不必按系统 DPI 缩放换算）、注入的事件保证被一帧消化完
//! 才返回，随后的 tree / shot 一定看得到结果。
//!
//! 应用需带 `EGUI_INSPECTION=1` 启动。用法：
//!   inspect tree [关键字]         列控件：角色 / 文本 / 逻辑坐标包围盒，给了关键字就只列匹配的
//!   inspect click <x> <y>         在逻辑坐标点一下
//!   inspect drag <x0> <y0> <x1> <y1>  按住左键从一点拖到另一点（选文本、拖分隔条）
//!   inspect copy                  发一次复制事件，选中的文本进系统剪贴板
//!   inspect scroll <x> <y> <dy>   在逻辑坐标滚一下（dy 正值内容下移）
//!   inspect type <文本>           往当前焦点控件敲一段文本
//!   inspect key <键>              敲一个键：enter / esc / tab / backspace / ctrl+a
//!   inspect shot <文件>           截图存 PNG

use std::net::TcpStream;

use egui::{Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, pos2, vec2};
use egui_inspection::protocol::{Request, Response, read_handshake, read_message, write_message};

const ADDR: &str = "127.0.0.1:5719";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("tree");
    match cmd {
        "tree" => tree(args.get(1).map(String::as_str)),
        "click" => {
            let pos = pos2(num(&args, 1)?, num(&args, 2)?);
            apply(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ])
        }
        "drag" => drag(
            pos2(num(&args, 1)?, num(&args, 2)?),
            pos2(num(&args, 3)?, num(&args, 4)?),
        ),
        "copy" => apply(vec![egui::Event::Copy]),
        "scroll" => {
            let (x, y, dy) = (num(&args, 1)?, num(&args, 2)?, num(&args, 3)?);
            apply(vec![
                egui::Event::PointerMoved(pos2(x, y)),
                egui::Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: vec2(0.0, dy),
                    phase: TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                },
            ])
        }
        "type" => apply(vec![egui::Event::Text(
            args.get(1).cloned().unwrap_or_default(),
        )]),
        "key" => {
            let (key, modifiers) = parse_key(args.get(1).map(String::as_str).unwrap_or(""))?;
            apply(vec![
                egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                },
                egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers,
                },
            ])
        }
        "shot" => shot(args.get(1).map(String::as_str).unwrap_or("shot.png")),
        other => {
            anyhow::bail!(
                "未知命令 {other}；可用：tree / click / drag / copy / scroll / type / key / shot"
            )
        }
    }
}

fn num(args: &[String], i: usize) -> anyhow::Result<f32> {
    args.get(i)
        .ok_or_else(|| anyhow::anyhow!("缺第 {i} 个参数"))?
        .parse()
        .map_err(Into::into)
}

/// 只认属性面板验收用得上的这几个键，不做通用键盘映射。
fn parse_key(name: &str) -> anyhow::Result<(Key, Modifiers)> {
    let (mods, bare) = match name.strip_prefix("ctrl+") {
        Some(rest) => (Modifiers::CTRL, rest),
        None => (Modifiers::NONE, name),
    };
    let key = match bare {
        "enter" => Key::Enter,
        "esc" => Key::Escape,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "a" => Key::A,
        other => anyhow::bail!("不认识的键 {other}"),
    };
    Ok((key, mods))
}

/// 每条命令开一条新连接：协议是一请求一应答，没有跨命令的会话状态。
fn request(req: &Request) -> anyhow::Result<Response> {
    let stream = TcpStream::connect(ADDR).map_err(|e| {
        anyhow::anyhow!("连不上 {ADDR}（应用是否带 EGUI_INSPECTION=1 启动？）：{e}")
    })?;
    read_handshake(&stream)?;
    write_message(&stream, req)?;
    let resp: Response = read_message(&stream)?;
    if let Response::Error { message } = &resp {
        anyhow::bail!("应用返回错误：{message}");
    }
    Ok(resp)
}

fn apply(events: Vec<egui::Event>) -> anyhow::Result<()> {
    request(&Request::ApplyEvents { events })?;
    println!("ok");
    Ok(())
}

/// 按住左键从 `from` 拖到 `to`。拆成三次请求发：拖动在 egui 里是跨帧状态机，
/// 按下 / 移动 / 松开挤进同一帧只会被当成一次点击，选不出文本。
fn drag(from: Pos2, to: Pos2) -> anyhow::Result<()> {
    request(&Request::ApplyEvents {
        events: vec![
            egui::Event::PointerMoved(from),
            egui::Event::PointerButton {
                pos: from,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    })?;
    request(&Request::ApplyEvents {
        events: vec![egui::Event::PointerMoved(to)],
    })?;
    request(&Request::ApplyEvents {
        events: vec![egui::Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    })?;
    println!("ok");
    Ok(())
}

fn tree(filter: Option<&str>) -> anyhow::Result<()> {
    let Response::Tree {
        step,
        pixels_per_point,
        accesskit,
    } = request(&Request::GetTree)?
    else {
        anyhow::bail!("应答类型不对");
    };
    let Some(update) = accesskit else {
        anyhow::bail!("应用还没产出 AccessKit 树");
    };
    println!(
        "step={step} ppp={pixels_per_point} nodes={}",
        update.nodes.len()
    );
    let mut shown = 0usize;
    for (id, node) in &update.nodes {
        let text = node.label().or_else(|| node.value()).unwrap_or("");
        if let Some(f) = filter
            && !text.contains(f)
        {
            continue;
        }
        let rect = node.bounds().map_or_else(
            || "-".to_owned(),
            |r| {
                format!(
                    "{:.0},{:.0} {:.0}x{:.0}",
                    r.x0,
                    r.y0,
                    r.x1 - r.x0,
                    r.y1 - r.y0
                )
            },
        );
        println!("{:>10} {:<16?} {:<22} {text}", id.0, node.role(), rect);
        shown += 1;
    }
    println!("matched {shown}");
    Ok(())
}

fn shot(path: &str) -> anyhow::Result<()> {
    // 按逻辑点分辨率截：出来的像素坐标与 tree 报的包围盒是同一套，
    // 采样点不用再按系统 DPI 换算。
    let Response::Screenshot(png) = request(&Request::GetScreenshot {
        pixels_per_point: Some(1.0),
    })?
    else {
        anyhow::bail!("应答类型不对");
    };
    std::fs::write(path, &png.bytes)?;
    println!("saved {path} {}x{}", png.size[0], png.size[1]);
    Ok(())
}
