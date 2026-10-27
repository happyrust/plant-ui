//! XLSX/CSV importer for the in-memory virtual-hole preview.
//!
//! 虚拟孔洞（ADR-0031）：一行一条记录，行级错误（坏 Shape、坏 Position、坏 Ori、
//! 尺寸非正）只进 `ImportReport::row_errors`，不阻断其他行；缺必需列才整表报错。
//! 原生端走 `import_file`（文件选择器拿到路径），浏览器端走 `import_bytes`
//! （隐藏 input 读出字节）——两者都落到同一个 `parse_rows`。

use anyhow::{Context, Result, bail};
use calamine::{Data, Reader, Xlsx};
use plant_ui::virtual_hole::{VirtualHole, VirtualHoleShape, parse_rely_refno};
use std::io::Cursor;
use std::path::Path;

/// 一次导入的结果：解析成功的孔洞 + 逐行错误。
#[derive(Debug, Default)]
pub struct ImportReport {
    pub holes: Vec<VirtualHole>,
    /// (来源行号, 错误信息)。行号从 1 开始（含表头行）。
    pub row_errors: Vec<(usize, String)>,
}

pub fn import_file(path: impl AsRef<Path>) -> Result<ImportReport> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)
        .with_context(|| format!("read {}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    import_bytes(&name, &bytes)
}

/// 按文件名后缀分流解析字节流。`name` 只用来取后缀（浏览器端没有路径）。
pub fn import_bytes(name: &str, bytes: &[u8]) -> Result<ImportReport> {
    match name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "csv" => {
            let text = String::from_utf8(bytes.to_vec()).context("CSV is not UTF-8")?;
            parse_csv(&text)
        }
        "xlsx" => {
            let mut book: Xlsx<_> = Xlsx::new(Cursor::new(bytes.to_vec()))
                .with_context(|| format!("open xlsx {name}"))?;
            let range = book
                .worksheet_range_at(0)
                .ok_or_else(|| anyhow::anyhow!("workbook has no sheets"))??;
            let rows: Vec<Vec<String>> =
                range.rows().map(|row| row.iter().map(cell).collect()).collect();
            parse_rows(rows)
        }
        other => bail!("unsupported virtual-hole file type: {other}"),
    }
}

fn cell(v: &Data) -> String {
    match v { Data::Empty => String::new(), Data::Float(x) => x.to_string(), Data::Int(x) => x.to_string(), Data::Bool(x) => x.to_string(), Data::String(x) => x.clone(), other => other.to_string() }
}

fn parse_csv(input: &str) -> Result<ImportReport> {
    let rows = input.lines().filter(|l| !l.trim().is_empty()).map(parse_csv_line).collect();
    parse_rows(rows)
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new(); let mut current = String::new(); let mut quoted = false;
    for ch in line.chars() { match ch { '"' => quoted = !quoted, ',' if !quoted => { out.push(current.trim().to_string()); current.clear(); }, _ => current.push(ch) } }
    out.push(current.trim().to_string()); out
}

fn parse_rows(rows: Vec<Vec<String>>) -> Result<ImportReport> {
    let Some(header) = rows.first() else { return Ok(ImportReport::default()); };
    let mut col = std::collections::HashMap::new();
    for (i, name) in header.iter().enumerate() { col.insert(name.trim().to_ascii_lowercase(), i); }
    // 必需列：Shape / Position / SizeThrowWall。缺哪列整表报错。
    for required in ["shape", "position", "sizethrowwall"] {
        if !col.contains_key(required) {
            bail!("missing required column: {required}");
        }
    }
    let get = |row: &Vec<String>, names: &[&str]| -> String { names.iter().find_map(|n| col.get(&n.to_ascii_lowercase()).and_then(|i| row.get(*i))).cloned().unwrap_or_default() };
    let mut report = ImportReport::default();
    for (index, row) in rows.iter().enumerate().skip(1) {
        let line = index + 1; // 含表头行
        if row.iter().all(|c| c.trim().is_empty()) { continue; }
        match parse_row(&get, row) {
            Ok(mut hole) => {
                hole.source_row = line;
                hole.rely_refno = parse_rely_refno(&hole.rely_item_ref);
                report.holes.push(hole);
            }
            Err(message) => report.row_errors.push((line, message)),
        }
    }
    Ok(report)
}

/// 解析一行；任何字段非法都返回行级错误（不中断其他行）。
fn parse_row(get: &dyn Fn(&Vec<String>, &[&str]) -> String, row: &Vec<String>) -> Result<VirtualHole, String> {
    let shape_raw = get(row, &["Shape"]);
    let shape = match shape_raw.trim().to_ascii_uppercase().as_str() {
        "CIR" | "CIRCLE" => VirtualHoleShape::Circle,
        "RECT" | "RECTANGLE" => VirtualHoleShape::Rect,
        other => return Err(format!("unsupported Shape {other:?}")),
    };
    let position = parse_vec3(&get(row, &["Position"]))?;
    let width = number(&get(row, &["SizeWidth"]))?;
    let height = number(&get(row, &["SizeHeight"]))?;
    let depth = number(&get(row, &["SizeThrowWall"]))?;
    let ori = get(row, &["Ori"]);
    let id = { let x = get(row, &["Code", "_key"]); if x.is_empty() { String::new() } else { x } };
    // 尺寸校验：圆孔要直径，矩形要宽和高，深度都得正——0 在这里就是错。
    match shape {
        VirtualHoleShape::Circle if !(width > 0.0) => {
            return Err("circle hole needs a positive SizeWidth (diameter)".into());
        }
        VirtualHoleShape::Rect if !(width > 0.0 && height > 0.0) => {
            return Err("rect hole needs positive SizeWidth and SizeHeight".into());
        }
        _ => {}
    }
    if !(depth > 0.0) {
        return Err("SizeThrowWall must be positive".into());
    }
    let orientation = parse_orientation(&ori)?;
    Ok(VirtualHole {
        id,
        shape,
        center_mm: position,
        orientation,
        diameter_mm: (shape == VirtualHoleShape::Circle).then_some(width),
        width_mm: (shape == VirtualHoleShape::Rect).then_some(width),
        height_mm: (shape == VirtualHoleShape::Rect).then_some(height),
        depth_mm: depth,
        rely_item_ref: get(row, &["RelyItemREF"]),
        rely_refno: None, // 由 parse_rows 补
        item_ref: get(row, &["ItemREF"]),
        source_row: 0, // 由 parse_rows 补
        hole_work: get(row, &["HoleWork"]),
        h_type: get(row, &["hType"]),
        subs_material: get(row, &["SubsMeterial", "SubsMaterial"]),
        raw_shape: shape_raw,
        raw_orientation: ori,
    })
}

fn number(value: &str) -> Result<f32, String> {
    let v = value.trim();
    if v.is_empty() { return Ok(0.); }
    v.parse().map_err(|_| format!("invalid number {v:?}"))
}

fn parse_vec3(value: &str) -> Result<[f32; 3], String> {
    let p: Vec<f32> = value.split(',').map(|x| x.trim().parse()).collect::<std::result::Result<_,_>>().map_err(|_| format!("invalid Position {value:?}"))?;
    if p.len() != 3 { return Err(format!("Position must contain three numbers, got {value:?}")); }
    Ok([p[0], p[1], p[2]])
}

/// Parse the E3D orientation strings (`<axis> is <dir> and <axis> is <dir>`). Any two of
/// X / Y / Z may be given (the sample file gives Y and Z); the third is derived by a
/// right-handed cross product. Directions accept both axis names and the PDMS compass
/// words (`N` = +Y, `E` = +X, `U` = +Z; `S` / `W` / `D` negative).
fn parse_orientation(value: &str) -> Result<[[f32; 3]; 3], String> {
    let mut x = None;
    let mut y = None;
    let mut z = None;
    for part in value.split(" and ") {
        let Some((lhs, rhs)) = part.split_once(" is ") else { continue };
        let v = axis(rhs.trim()).ok_or_else(|| format!("unsupported orientation {value:?}"))?;
        match lhs.trim().to_ascii_uppercase().as_str() {
            "X" => x = Some(v),
            "Y" => y = Some(v),
            "Z" => z = Some(v),
            _ => {}
        }
    }
    // 右手系补第三根轴：给了哪两根就叉乘出剩下那根。
    let (x, y, z) = match (x, y, z) {
        (Some(x), Some(y), Some(z)) => (x, y, z),
        (None, Some(y), Some(z)) => (y.cross(z), y, z),
        (Some(x), None, Some(z)) => (x, z.cross(x), z),
        (Some(x), Some(y), None) => (x, y, x.cross(y)),
        _ => {
            return Err(format!(
                "orientation needs two axes (e.g. `Y is N and Z is U`): {value:?}"
            ));
        }
    };
    if [x, y, z].iter().any(|axis| axis.length_squared() < 0.5) {
        return Err(format!("orientation axes are parallel: {value:?}"));
    }
    Ok([
        x.normalize_or_zero().to_array(),
        y.normalize_or_zero().to_array(),
        z.normalize_or_zero().to_array(),
    ])
}

/// 一个方向词：轴名（`X` / `-Y` …）或 PDMS / E3D 的罗盘写法（`N` `S` `E` `W` `U` `D`，也认带负号的）。
fn axis(value: &str) -> Option<glam::Vec3> {
    Some(match value.trim().to_ascii_uppercase().as_str() {
        "X" | "E" | "-W" => glam::Vec3::X,
        "-X" | "W" | "-E" => -glam::Vec3::X,
        "Y" | "N" | "-S" => glam::Vec3::Y,
        "-Y" | "S" | "-N" => -glam::Vec3::Y,
        "Z" | "U" | "-D" => glam::Vec3::Z,
        "-Z" | "D" | "-U" => -glam::Vec3::Z,
        _ => return None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 测试内拼一个最小 xlsx（内存 zip）：表头 + 3 条记录。
    /// CIR Φ325、CIR Φ300、RECT 300×400，深度均 800，姿态 `Y is Y and Z is -Z`。
    pub(crate) fn fixture_xlsx() -> Vec<u8> {
        let header = [
            "Code", "Position", "Shape", "SizeWidth", "SizeHeight", "SizeThrowWall", "Ori",
            "RelyItemREF", "ItemREF", "SubsMaterial",
        ];
        let rows: Vec<Vec<String>> = [
            vec!["HOLE-001", "10500,3200,-450", "CIR", "325", "0", "800", "Y is Y and Z is -Z", "24381_145018", "ITEM-1", "STEEL"],
            vec!["HOLE-002", "11200,3200,-450", "CIR", "300", "0", "800", "Y is Y and Z is -Z", "24381/145019", "ITEM-2", "STEEL"],
            vec!["HOLE-003", "12000,3200,-450", "RECT", "300", "400", "800", "Y is Y and Z is -Z", "=24381_145020", "ITEM-3", "CONC"],
        ]
        .into_iter()
        .map(|r| r.into_iter().map(str::to_string).collect())
        .collect();
        let mut all = vec![header.iter().map(|s| s.to_string()).collect()];
        all.extend(rows);
        let sheet = xlsx_sheet_xml(&all);
        build_xlsx(&sheet)
    }

    fn col_letter(index: usize) -> String {
        let mut n = index + 1;
        let mut s = String::new();
        while n > 0 {
            let rem = (n - 1) % 26;
            s.insert(0, (b'A' + rem as u8) as char);
            n = (n - 1) / 26;
        }
        s
    }

    /// 用 inlineStr 写死字符串、数字写 <v> 的最小 sheet1.xml。rows 是「行 → 列」。
    fn xlsx_sheet_xml(rows: &[Vec<String>]) -> String {
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>",
        );
        for (r, line) in rows.iter().enumerate() {
            xml.push_str(&format!("<row r=\"{}\">", r + 1));
            for (c, field) in line.iter().enumerate() {
                let ref_ = format!("{}{}", col_letter(c), r + 1);
                if let Ok(v) = field.parse::<f64>() {
                    xml.push_str(&format!("<c r=\"{ref_}\"><v>{v}</v></c>"));
                } else {
                    let escaped = field
                        .replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;");
                    xml.push_str(&format!(
                        "<c r=\"{ref_}\" t=\"inlineStr\"><is><t>{escaped}</t></is></c>"
                    ));
                }
            }
            xml.push_str("</row>");
        }
        xml.push_str("</sheetData></worksheet>");
        xml
    }

    /// 最小 xlsx：五个部件打进一个 zip（deflate）。
    fn build_xlsx(sheet_xml: &str) -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("[Content_Types].xml", options).unwrap();
            zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#).unwrap();
            zip.start_file("_rels/.rels", options).unwrap();
            zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#).unwrap();
            zip.start_file("xl/workbook.xml", options).unwrap();
            zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#).unwrap();
            zip.start_file("xl/_rels/workbook.xml.rels", options).unwrap();
            zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#).unwrap();
            zip.start_file("xl/worksheets/sheet1.xml", options).unwrap();
            zip.write_all(sheet_xml.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn csv_fixture_parses() {
        let input = "Code,Position,Shape,SizeWidth,SizeHeight,SizeThrowWall,Ori,RelyItemREF,ItemREF\nh,\"1,2,3\",CIR,300,0,800,\"Y is Y and Z is -Z\",r,i";
        let report = parse_csv(input).unwrap();
        assert_eq!(report.holes.len(), 1);
        assert!(report.row_errors.is_empty());
    }

    #[test]
    fn xlsx_fixture_parses_three_rows() {
        let bytes = fixture_xlsx();
        let report = import_bytes("holes.xlsx", &bytes).unwrap();
        assert_eq!(report.holes.len(), 3, "row_errors: {:?}", report.row_errors);
        assert!(report.row_errors.is_empty());
        let [a, b, c] = &report.holes[..] else { panic!() };
        assert_eq!(a.id, "HOLE-001");
        assert_eq!(a.diameter_mm, Some(325.0));
        assert_eq!(b.diameter_mm, Some(300.0));
        assert_eq!(c.shape, VirtualHoleShape::Rect);
        assert_eq!((c.width_mm, c.height_mm), (Some(300.0), Some(400.0)));
        assert_eq!(a.depth_mm, 800.0);
        // Position 三轴解析。
        assert_eq!(a.center_mm, [10500.0, 3200.0, -450.0]);
        // 姿态 `Y is Y and Z is -Z`：Y=(0,1,0)，Z=(0,0,-1)，X=Y×Z=(−1,0,0)。
        assert_eq!(a.orientation[0], [-1.0, 0.0, 0.0]);
        assert_eq!(a.orientation[1], [0.0, 1.0, 0.0]);
        assert_eq!(a.orientation[2], [0.0, 0.0, -1.0]);
        // 来源行号（含表头）与参考号三种写法。
        assert_eq!(a.source_row, 2);
        assert_eq!(c.source_row, 4);
        assert_eq!(a.rely_refno, parse_rely_refno("24381_145018"));
        assert_eq!(b.rely_refno, parse_rely_refno("24381/145019"));
        assert_eq!(c.rely_refno, parse_rely_refno("=24381_145020"));
        // SubsMaterial 列名（也认 SubsMeterial 拼残）。
        assert_eq!(c.subs_material, "CONC");
        // ItemREF / Code 保留。
        assert_eq!(a.item_ref, "ITEM-1");
    }

    #[test]
    fn bad_rows_do_not_block_good_ones() {
        let input = "Code,Position,Shape,SizeWidth,SizeHeight,SizeThrowWall,Ori\n\
                     ok,\"1,2,3\",CIR,300,0,800,\"Y is Y and Z is -Z\"\n\
                     bad-shape,\"1,2,3\",OVAL,300,0,800,\"Y is Y and Z is -Z\"\n\
                     bad-pos,\"1,2\",CIR,300,0,800,\"Y is Y and Z is -Z\"\n\
                     zero-depth,\"1,2,3\",CIR,300,0,0,\"Y is Y and Z is -Z\"\n\
                     zero-diameter,\"1,2,3\",CIR,0,0,800,\"Y is Y and Z is -Z\"\n\
                     ok2,\"4,5,6\",RECT,300,400,800,\"Y is Y and Z is -Z\"";
        let report = parse_csv(input).unwrap();
        assert_eq!(report.holes.len(), 2);
        assert_eq!(report.row_errors.len(), 4);
        assert_eq!(report.row_errors[0].0, 3); // 来源行号从 1 开始（含表头）
        assert!(report.row_errors.iter().all(|(_, m)| !m.is_empty()));
    }

    #[test]
    fn circle_hole_with_zero_height_is_valid() {
        let input = "Position,Shape,SizeWidth,SizeHeight,SizeThrowWall,Ori\n\"1,2,3\",CIR,300,0,800,\"Y is Y and Z is -Z\"";
        let report = parse_csv(input).unwrap();
        assert_eq!(report.holes.len(), 1);
        assert_eq!(report.holes[0].height_mm, None); // 圆孔不读 SizeHeight
    }

    #[test]
    fn missing_required_column_fails_the_table() {
        let input = "Code,Shape\nh,CIR";
        assert!(parse_csv(input).is_err());
    }

    #[test]
    fn leading_zero_code_is_preserved() {
        let input = "Code,Position,Shape,SizeWidth,SizeHeight,SizeThrowWall,Ori\n007,\"1,2,3\",CIR,300,0,800,\"Y is Y and Z is -Z\"";
        let report = parse_csv(input).unwrap();
        assert_eq!(report.holes[0].id, "007");
    }

    #[test]
    fn subs_meterial_misspelling_is_accepted() {
        let input = "Position,Shape,SizeWidth,SizeHeight,SizeThrowWall,Ori,SubsMeterial\n\"1,2,3\",CIR,300,0,800,\"Y is Y and Z is -Z\",C30";
        let report = parse_csv(input).unwrap();
        assert_eq!(report.holes[0].subs_material, "C30");
    }

    #[test]
    fn unsupported_extension_is_rejected() {
        assert!(import_bytes("holes.txt", b"x").is_err());
    }

    /// PDMS / E3D 的方向串常用罗盘写法（N=+Y、E=+X、U=+Z，S / W / D 取负）；给的是哪两根轴都能补出第三根（右手系）。
    #[test]
    fn compass_orientation_and_any_two_axes_are_accepted() {
        let ori = parse_orientation("Y is N and Z is U").unwrap();
        assert_eq!(ori, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        let ori = parse_orientation("X is E and Z is D").unwrap();
        assert_eq!(ori, [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]]);
        let ori = parse_orientation("X is -Y and Y is X").unwrap();
        assert_eq!(ori, [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
        assert!(
            parse_orientation("Y is N and Z is N").is_err(),
            "两根轴平行推不出第三根"
        );
        assert!(parse_orientation("Y is N").is_err(), "只给一根轴不够");
        assert!(parse_orientation("Y is NE and Z is U").is_err(), "方向词认不出就是行级错误");
    }
}
