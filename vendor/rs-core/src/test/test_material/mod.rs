
//todo 各个专业按照 SITE 整理不同的测试案例
//todo 收集的不同专业的结果，对应 SITE 收集有问题的模型

use crate::material::define_surreal_functions;
use crate::{RefU64, SUL_DB};
use crate::material::gy::save_gy_material_dzcl;

async fn test_gen_ms(refno: RefU64){
    crate::init_test_surreal().await;
    if let Err(e) = define_surreal_functions(SUL_DB.clone()).await {
        dbg!(e.to_string());
        // return Ok(());
        return;
    }

    let handles = save_gy_material_dzcl(refno).await;
    futures::prelude::future::join_all(handles).await;
}


//fix Cannot perform multiplication with '1.57075' and 'NONE'
#[tokio::test]
async fn test_perform_multiplication_with_none(){
    let gy_refno: RefU64 = "24383/66457".into();
    test_gen_ms(gy_refno).await;
}