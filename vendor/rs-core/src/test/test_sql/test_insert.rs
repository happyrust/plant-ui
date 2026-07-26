use sea_orm::ConnectionTrait;
use crate::named_attmap::NamedAttrMap;
use crate::named_attvalue::NamedAttrValue;
use crate::test::test_sql::get_version_conn;

#[tokio::test]
async fn test_gen_insert_sql() {
    // let create_sql = db_info.gen_create_table_sql("BOX").unwrap();
    // dbg!(&create_sql);
    let mut pipe_att = NamedAttrMap::new("PIPE");
    pipe_att.insert("REFNO".to_string(), NamedAttrValue::StringType("10/1".into()));
    pipe_att.insert("OWNER".to_string(), NamedAttrValue::StringType("1/0".into()));
    // att.insert("REFNO".to_string(), NamedAttrValue::StringType("1/0".into()));
    // dbg!(&pipe_att);
    let sql = pipe_att.gen_insert_sql(true).unwrap();
    // dbg!(&sql);

    let mut box_att = NamedAttrMap::new("BOX");
    box_att.insert("REFNO".to_string(), NamedAttrValue::StringType("10/2".into()));
    box_att.insert("OWNER".to_string(), NamedAttrValue::StringType("1/0".into()));

    let mut final_sqls = NamedAttrMap::gen_insert_many_sql([pipe_att.clone(), box_att.clone()], true).unwrap();
    // dbg!(&final_sqls);

    let db = get_version_conn().await;
    NamedAttrMap::exec_insert_many([pipe_att, box_att], &db, true).await.unwrap();

    NamedAttrMap::exec_commit_atts_change(&db,Some("Save test atts")).await.unwrap();
    // db.execute_unprepared(&final_sqls).await.unwrap();
    // box_att.exec_insert(&db).await.unwrap();
}