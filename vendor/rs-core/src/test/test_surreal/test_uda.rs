use crate::rs_surreal;



#[tokio::test]
async fn test_query_uda_name() {
    crate::init_test_surreal().await;
    let hash = 397059875;
    let refno = crate::uda::get_uda_refno(hash).await.unwrap();
    dbg!(refno);
    let name = crate::uda::get_uda_name(hash).await.unwrap();
    dbg!(name);

    let vt = crate::uda::get_uda_type(hash).await.unwrap();
    dbg!(vt);

}

