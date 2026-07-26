use crate::get_default_pdms_db_info;
use crate::test::test_sql::get_version_conn;

#[test]
fn test_create_table() {
    let db_info = get_default_pdms_db_info();

    let create_sql = db_info.gen_create_table_sql("BOX").unwrap();
    dbg!(&create_sql);
}

#[tokio::test]
async fn test_query_diff() {

    let db = get_version_conn().await;
    //执行git add 的操作
    db.execute_unprepared(r#"call dolt_add('.')"#).await.unwrap();
    db.execute_unprepared(r#"call dolt_commit('-m', 'Inited commit.')"#).await;


    //数据都需要支持转回NamedAttrMap的功能
    //DynamicStruct 转回Map的功能，对外的接口还是

    //模拟修改，提交的动作，然后查询出来差异
    //模拟PdmsElement 的版本号的修改
    use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};
    use crate::options::DbOption;
    use crate::orm::diff_data::{Column, Entity};
    use crate::orm::pdms_element;
    let mut ele = pdms_element::ActiveModel{
        id: Set("17496/100001".into()),
        status_tag: Set(Some("D0".to_string())),
        ..Default::default()
    };
    //修改tag
    let mut new_ele = ele.save(&db).await.unwrap();
    new_ele.status_tag = Set("D2".to_string().into());
    new_ele.save(&db).await.unwrap();

    db.execute_unprepared(r#"call dolt_add('PdmsElement')"#).await.unwrap();
    db.execute_unprepared(r#"call dolt_commit('-am', 'Status changed.')"#).await;

    let results = Entity::find()
        .filter(Column::TableName.eq("PdmsElement"))
        .all(&db)
        .await.unwrap();
    dbg!(&results);


}
