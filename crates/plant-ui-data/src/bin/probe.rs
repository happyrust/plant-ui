//! 数据层验收探针：直连本地 SurrealDB，覆盖 M0-4 采样、M1-2 模型树查询与
//! M1-3 元素属性。运行：cargo run -p plant-ui-data --bin probe（工作目录需有 DbOption.toml）

use plant_ui_data::AttrKind;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    plant_ui_data::connect().await?;

    let (project, mdb, ns, db_nums) = plant_ui_data::project_identity().await?;
    println!("project={project} mdb={mdb} ns={ns} desi_dbs={db_nums:?}");

    let rows = plant_ui_data::sample_named_elements(20).await?;
    println!("sample_named_elements -> {} rows", rows.len());
    for (refno, name, noun) in rows.iter().take(5) {
        println!("  {refno:<16} {noun:<6} {name}");
    }

    let sites = plant_ui_data::site_nodes().await?;
    println!("site_nodes -> {} rows", sites.len());
    for s in &sites {
        println!(
            "  {:<12} {:<6} {:<20} children={}",
            s.refno.refno().to_string(),
            s.noun,
            s.name,
            s.children_count
        );
    }

    if let Some(first) = sites.first() {
        let kids = plant_ui_data::child_nodes(first.refno).await?;
        println!("child_nodes({}) -> {} rows", first.name, kids.len());
        for k in kids.iter().take(5) {
            println!(
                "  {:<12} {:<6} {:<20} order={} children={}",
                k.refno.refno().to_string(),
                k.noun,
                k.name,
                k.order,
                k.children_count
            );
        }

        // M1-3：属性面板的数据源。沿树往下走一层再取，叶子层的属性才有代表性
        // （SITE 只有寥寥几个属性，看不出分组与 unset 的处理）。
        let target = match kids.first() {
            Some(k) => plant_ui_data::child_nodes(k.refno)
                .await?
                .first()
                .map_or(k.refno, |g| g.refno),
            None => first.refno,
        };
        let props = plant_ui_data::element_props(target).await?;
        let udas = props.iter().filter(|a| a.name.starts_with(':')).count();
        let unset = props.iter().filter(|a| a.value == "unset").count();
        let editable = props
            .iter()
            .filter(|a| !matches!(a.kind, AttrKind::Unset | AttrKind::Opaque))
            .count();
        // 值格空白是「空值有明确显示」的反例：既看不出是空值还是没查到。
        // 空数组曾从这里漏出去（每个 EQUI / NOZZ 都带一行空 DESP）。
        let blank = props.iter().filter(|a| a.value.trim().is_empty()).count();
        println!(
            "element_props({}) -> {} 项（UDA {} / unset {} / 可编辑 {} / 空值格 {}）",
            target.refno(),
            props.len(),
            udas,
            unset,
            editable,
            blank
        );
        // 有值的行全列出来，只读的也列：可编辑字段被错判成只读时，光看可编辑那一批
        // 是看不出来的——ISOH / LOOS 当初就是这么漏过去的。
        for a in props.iter().filter(|a| a.value != "unset") {
            println!("  {:<12} {:<28} {:?}", a.name, a.value, a.kind);
        }
    }
    Ok(())
}
