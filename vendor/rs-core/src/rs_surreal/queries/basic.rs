//! 基础查询模块
//! 
//! 提供最基本的数据库查询功能，包括单个元素查询、类型查询等。

use crate::pe::SPdmsElement;
use crate::rs_surreal::cache_manager::QUERY_CACHE;
use crate::rs_surreal::error_handler::{QueryError, QueryErrorHandler, QueryResult};
use crate::rs_surreal::query_builder::{FunctionQueryBuilder, PeQueryBuilder};
use crate::types::*;
use anyhow::Result;
use cached::proc_macro::cached;
use std::time::Instant;

/// 基础查询服务
pub struct BasicQueryService;

impl BasicQueryService {
    /// 通过 refno 查询 PE 数据
    /// 
    /// # 参数
    /// * `refno` - 要查询的参考号
    /// 
    /// # 返回值
    /// * `Result<Option<SPdmsElement>>` - 查询结果，如果找到则返回 PE 数据，否则返回 None
    /// 
    /// # 错误
    /// * 如果数据库查询失败会返回错误
    /// 
    /// # 示例
    /// ```rust
    /// use crate::rs_surreal::queries::basic::BasicQueryService;
    /// use crate::types::RefnoEnum;
    /// 
    /// let refno = RefnoEnum::from("12345_67890");
    /// let pe = BasicQueryService::get_pe(refno).await?;
    /// ```
    pub async fn get_pe(refno: RefnoEnum) -> Result<Option<SPdmsElement>> {
        let start_time = Instant::now();
        
        // 先检查缓存
        if let Some(cached_pe) = QUERY_CACHE.get_pe(&refno).await {
            return Ok(Some(cached_pe));
        }

        // 构建查询
        let query = PeQueryBuilder::new(refno).basic_query();
        let sql = query.build().to_string();

        // 执行查询
        match query.fetch_one::<SPdmsElement>().await {
            Ok(pe_opt) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);

                // 缓存结果
                if let Some(ref pe) = pe_opt {
                    QUERY_CACHE.set_pe(refno, pe.clone()).await;
                }

                Ok(pe_opt)
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }

    /// 获取默认名称
    /// 
    /// # 参数
    /// * `refno` - 要查询的参考号
    /// 
    /// # 返回值
    /// * `Result<Option<String>>` - 默认名称，如果未找到则返回 None
    /// 
    /// # 错误
    /// * 如果数据库查询失败会返回错误
    pub async fn get_default_name(refno: RefnoEnum) -> Result<Option<String>> {
        let start_time = Instant::now();
        let query = FunctionQueryBuilder::default_name(refno);
        let sql = query.build().to_string();

        match query.fetch_one::<String>().await {
            Ok(name) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);
                Ok(name)
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }

    /// 获取类型名称
    /// 
    /// # 参数
    /// * `refno` - 要查询的参考号
    /// 
    /// # 返回值
    /// * `Result<String>` - 类型名称，如果未找到则返回 "unset"
    /// 
    /// # 错误
    /// * 如果数据库查询失败会返回错误
    pub async fn get_type_name(refno: RefnoEnum) -> Result<String> {
        let start_time = Instant::now();
        
        // 先检查缓存
        if let Some(cached_type) = QUERY_CACHE.get_type_name(&refno).await {
            return Ok(cached_type);
        }

        // 构建查询
        let query = PeQueryBuilder::new(refno).type_query();
        let sql = query.build().to_string();

        // 执行查询
        match query.fetch_one::<String>().await {
            Ok(type_name_opt) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);

                let type_name = type_name_opt.unwrap_or_else(|| "unset".to_string());
                
                // 缓存结果
                QUERY_CACHE.set_type_name(refno, type_name.clone()).await;

                Ok(type_name)
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }

    /// 获取默认全名
    /// 
    /// # 参数
    /// * `refno` - 要查询的参考号
    /// 
    /// # 返回值
    /// * `Result<String>` - 默认全名
    /// 
    /// # 错误
    /// * 如果数据库查询失败会返回错误
    pub async fn get_default_full_name(refno: RefnoEnum) -> Result<String> {
        let start_time = Instant::now();
        let query = FunctionQueryBuilder::default_full_name(refno);
        let sql = query.build().to_string();

        match query.fetch_one::<String>().await {
            Ok(name_opt) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);
                Ok(name_opt.unwrap_or_default())
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }

    /// 通过名称查询 refno
    /// 
    /// # 参数
    /// * `name` - 要查询的名称
    /// 
    /// # 返回值
    /// * `Result<Option<RefnoEnum>>` - 如果找到则返回对应的 refno，否则返回 None
    /// 
    /// # 错误
    /// * 如果数据库查询失败会返回错误
    pub async fn get_refno_by_name(name: &str) -> Result<Option<RefnoEnum>> {
        use crate::rs_surreal::query_builder::QueryBuilder;
        
        let start_time = Instant::now();
        let sql = format!(
            r#"SELECT value id FROM ONLY pe WHERE name="/{}" LIMIT 1"#,
            name
        );
        
        let query = QueryBuilder::from_sql(&sql);

        match query.fetch_one::<RefnoEnum>().await {
            Ok(refno) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);
                Ok(refno)
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }

    /// 批量获取类型名称
    /// 
    /// # 参数
    /// * `refnos` - refno 迭代器
    /// 
    /// # 返回值
    /// * `Result<Vec<String>>` - 类型名称列表
    /// 
    /// # 错误
    /// * 如果数据库查询失败会返回错误
    pub async fn get_type_names(
        refnos: impl Iterator<Item = &RefnoEnum>,
    ) -> Result<Vec<String>> {
        use crate::rs_surreal::query_builder::BatchQueryBuilder;
        use itertools::Itertools;
        
        let start_time = Instant::now();
        let refno_vec: Vec<RefnoEnum> = refnos.cloned().collect();
        
        if refno_vec.is_empty() {
            return Ok(Vec::new());
        }

        let query = BatchQueryBuilder::new(refno_vec).types_query();
        let sql = query.build().to_string();

        match query.fetch_all::<String>().await {
            Ok(type_names) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);
                QueryErrorHandler::log_query_results(&sql, type_names.len());
                Ok(type_names)
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }

    /// 检查记录是否存在
    /// 
    /// # 参数
    /// * `refno` - 要检查的参考号
    /// 
    /// # 返回值
    /// * `Result<bool>` - 如果记录存在则返回 true，否则返回 false
    /// 
    /// # 错误
    /// * 如果数据库查询失败会返回错误
    pub async fn exists(refno: RefnoEnum) -> Result<bool> {
        use crate::rs_surreal::query_builder::QueryBuilder;
        
        let start_time = Instant::now();
        let sql = format!(
            "SELECT value record::exists({})",
            refno.to_pe_key()
        );
        
        let query = QueryBuilder::from_sql(&sql);

        match query.fetch_one::<bool>().await {
            Ok(exists) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);
                Ok(exists.unwrap_or(false))
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }
}

/// 缓存版本的基础查询函数（保持向后兼容）
#[cached(result = true)]
pub async fn get_pe(refno: RefnoEnum) -> anyhow::Result<Option<SPdmsElement>> {
    BasicQueryService::get_pe(refno).await
}

#[cached(result = true)]
pub async fn get_type_name(refno: RefnoEnum) -> anyhow::Result<String> {
    BasicQueryService::get_type_name(refno).await
}

pub async fn get_default_name(refno: RefnoEnum) -> anyhow::Result<Option<String>> {
    BasicQueryService::get_default_name(refno).await
}

pub async fn get_refno_by_name(name: &str) -> anyhow::Result<Option<RefnoEnum>> {
    BasicQueryService::get_refno_by_name(name).await
}

pub async fn get_type_names(
    refnos: impl Iterator<Item = &RefnoEnum>,
) -> anyhow::Result<Vec<String>> {
    BasicQueryService::get_type_names(refnos).await
}

#[cached(result = true)]
pub async fn get_default_full_name(refno: RefnoEnum) -> anyhow::Result<String> {
    BasicQueryService::get_default_full_name(refno).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_query_service() {
        // 这里需要实际的数据库连接来测试
        // 暂时只测试函数签名和基本逻辑
        let refno = RefnoEnum::from("12345_67890");
        
        // 测试类型名称查询的错误处理
        match BasicQueryService::get_type_name(refno).await {
            Ok(_) => {
                // 查询成功
            }
            Err(_) => {
                // 预期的错误，因为没有实际的数据库连接
            }
        }
    }
}
