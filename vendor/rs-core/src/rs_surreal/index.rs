use crate::{options::DbOption, SUL_DB};

///创建几何相关索引索引
pub async fn create_geom_index() -> anyhow::Result<()> {
    //针对一些特殊的表，需要先创建表，定义索引
    //DEFINE INDEX unique_geo_relate ON TABLE geo_relate COLUMNS in, geom_refno UNIQUE;
    // DEFINE INDEX unique_tubi_relate ON TABLE tubi_relate COLUMNS arrive, leave UNIQUE
    //DEFINE INDEX unique_inst_relate ON TABLE inst_relate COLUMNS in, out UNIQUE;
    SUL_DB
        .query(
            r#"
                DEFINE INDEX unique_neg_relate ON TABLE neg_relate COLUMNS in, out UNIQUE;
                DEFINE INDEX unique_nearest_relate ON TABLE nearest_relate COLUMNS in, out UNIQUE;
             "#,
        )
        .await
        .unwrap();
    Ok(())
}

pub async fn define_room_index() -> anyhow::Result<()> {
    //针对一些特殊的表，需要先创建表，定义索引
    SUL_DB
        .query(
            r#"
        DEFINE INDEX unique_room_relate ON TABLE room_relate COLUMNS in, out UNIQUE;
        DEFINE INDEX unique_room_panel_relate ON TABLE room_panel_relate COLUMNS in, out UNIQUE;
               "#,
        )
        .await
        .unwrap();
    Ok(())
}

/// 创建 pe_owner 的唯一性索引，in, out的组合索引
pub async fn define_owner_index() -> anyhow::Result<()> {
    //针对一些特殊的表，需要先创建表，定义索引
    SUL_DB
        .query(r#"DEFINE INDEX unique_pe_owner ON TABLE pe_owner COLUMNS in, out UNIQUE"#)
        .await
        .unwrap();
    Ok(())
}

pub async fn define_fullname_index() -> anyhow::Result<()> {
    //针对一些特殊的表，需要先创建表，定义索引
    SUL_DB
        .query(r#"DEFINE ANALYZER name_fulltext TOKENIZERS class FILTERS lowercase;
                    DEFINE INDEX fulltext_name ON TABLE pe FIELDS name SEARCH ANALYZER name_fulltext BM25 HIGHLIGHTS;
                "#)
        .await
        .unwrap();
    Ok(())
}

pub async fn define_pe_index() -> anyhow::Result<()> {
    //针对一些特殊的表，需要先创建表，定义索引
    SUL_DB
        .query(
            r#"
        DEFINE index pe_name_index ON TABLE pe COLUMNS name;
        DEFINE index pe_refno_index ON TABLE pe COLUMNS refno;
        DEFINE index pe_cata_hash_index ON TABLE pe COLUMNS cata_hash;
        DEFINE index pe_dbnum_index ON TABLE pe COLUMNS dbnum;
        DEFINE INDEX sesno_index ON TABLE pe COLUMNS sesno;
                "#,
        )
        .await
        .unwrap();
    Ok(())
}
pub async fn define_ses_index() -> anyhow::Result<()> {
    //针对一些特殊的表，需要先创建表，定义索引
    SUL_DB
        .query(
            r#"
        DEFINE INDEX date_index ON ses COLUMNS date;
        DEFINE INDEX dbnum_index ON ses COLUMNS dbnum;
        DEFINE INDEX sesno_index ON ses COLUMNS sesno;
                "#,
        )
        .await
        .unwrap();
    Ok(())
}
