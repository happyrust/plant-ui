//! 查询错误处理模块
//! 
//! 提供统一的错误处理机制，包括错误日志记录、错误转换和错误恢复策略。

use crate::error::HandleError;
use anyhow::{anyhow, Result};
use log::{error, warn, debug};
use std::fmt::Display;
use surrealdb::Error as SurrealError;

/// 查询错误类型
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("Database connection error: {message}")]
    ConnectionError { message: String },
    
    #[error("Query execution error: {sql} - {message}")]
    ExecutionError { sql: String, message: String },
    
    #[error("Deserialization error for {type_name}: {message}")]
    DeserializationError { type_name: String, message: String },
    
    #[error("Query returned no results: {sql}")]
    NoResultsError { sql: String },
    
    #[error("Invalid query parameters: {message}")]
    InvalidParametersError { message: String },
    
    #[error("Cache error: {message}")]
    CacheError { message: String },
    
    #[error("Timeout error: query took too long to execute")]
    TimeoutError,
}

impl From<SurrealError> for QueryError {
    fn from(error: SurrealError) -> Self {
        match error {
            SurrealError::Db(db_error) => QueryError::ExecutionError {
                sql: "unknown".to_string(),
                message: db_error.to_string(),
            },
            _ => QueryError::ConnectionError {
                message: error.to_string(),
            },
        }
    }
}

impl From<QueryError> for HandleError {
    fn from(error: QueryError) -> Self {
        match error {
            QueryError::ConnectionError { message } => HandleError::SurrealError { msg: message },
            QueryError::ExecutionError { sql, message } => HandleError::QueryErr {
                sql,
                msg: message,
                position: std::panic::Location::caller().to_string(),
            },
            QueryError::DeserializationError { type_name, message } => HandleError::DeserializeErr {
                struct_name: type_name,
                msg: message,
                sql: "unknown".to_string(),
                position: std::panic::Location::caller().to_string(),
            },
            QueryError::NoResultsError { sql } => HandleError::QueryNullErr {
                sql,
                position: std::panic::Location::caller().to_string(),
            },
            _ => HandleError::SurrealError {
                msg: error.to_string(),
            },
        }
    }
}

/// 查询错误处理器
pub struct QueryErrorHandler;

impl QueryErrorHandler {
    /// 处理查询执行错误
    pub fn handle_execution_error(sql: &str, error: impl Display) -> QueryError {
        let error_msg = error.to_string();
        error!("Query execution failed: {} - Error: {}", sql, error_msg);
        
        QueryError::ExecutionError {
            sql: sql.to_string(),
            message: error_msg,
        }
    }

    /// 处理反序列化错误
    pub fn handle_deserialization_error<T>(error: impl Display) -> QueryError {
        let type_name = std::any::type_name::<T>();
        let error_msg = error.to_string();
        error!("Deserialization failed for {}: {}", type_name, error_msg);
        
        QueryError::DeserializationError {
            type_name: type_name.to_string(),
            message: error_msg,
        }
    }

    /// 处理空结果错误
    pub fn handle_no_results_error(sql: &str) -> QueryError {
        warn!("Query returned no results: {}", sql);
        
        QueryError::NoResultsError {
            sql: sql.to_string(),
        }
    }

    /// 处理连接错误
    pub fn handle_connection_error(error: impl Display) -> QueryError {
        let error_msg = error.to_string();
        error!("Database connection error: {}", error_msg);
        
        QueryError::ConnectionError {
            message: error_msg,
        }
    }

    /// 处理缓存错误
    pub fn handle_cache_error(error: impl Display) -> QueryError {
        let error_msg = error.to_string();
        warn!("Cache error: {}", error_msg);
        
        QueryError::CacheError {
            message: error_msg,
        }
    }

    /// 记录查询执行信息
    pub fn log_query_execution(sql: &str, duration_ms: u64) {
        if duration_ms > 1000 {
            warn!("Slow query detected ({}ms): {}", duration_ms, sql);
        } else {
            debug!("Query executed ({}ms): {}", duration_ms, sql);
        }
    }

    /// 记录查询结果统计
    pub fn log_query_results(sql: &str, result_count: usize) {
        debug!("Query returned {} results: {}", result_count, sql);
    }
}

/// 查询结果包装器，提供错误处理和日志记录
pub struct QueryResult<T> {
    result: Result<T>,
    sql: String,
    execution_time_ms: Option<u64>,
}

impl<T> QueryResult<T> {
    /// 创建成功的查询结果
    pub fn success(result: T, sql: String, execution_time_ms: Option<u64>) -> Self {
        if let Some(time) = execution_time_ms {
            QueryErrorHandler::log_query_execution(&sql, time);
        }
        
        Self {
            result: Ok(result),
            sql,
            execution_time_ms,
        }
    }

    /// 创建失败的查询结果
    pub fn error(error: QueryError, sql: String) -> Self {
        error!("Query failed: {} - Error: {}", sql, error);
        
        Self {
            result: Err(anyhow!(error)),
            sql,
            execution_time_ms: None,
        }
    }

    /// 获取结果，如果失败则记录错误
    pub fn into_result(self) -> Result<T> {
        self.result
    }

    /// 获取结果，如果失败则返回默认值
    pub fn unwrap_or_default(self) -> T
    where
        T: Default,
    {
        match self.result {
            Ok(value) => value,
            Err(error) => {
                warn!("Query failed, using default value: {}", error);
                T::default()
            }
        }
    }

    /// 获取结果，如果失败则使用提供的默认值
    pub fn unwrap_or(self, default: T) -> T {
        match self.result {
            Ok(value) => value,
            Err(error) => {
                warn!("Query failed, using provided default: {}", error);
                default
            }
        }
    }

    /// 获取 SQL 字符串
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// 获取执行时间
    pub fn execution_time_ms(&self) -> Option<u64> {
        self.execution_time_ms
    }
}

/// 重试策略配置
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: usize,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 100,
            max_delay_ms: 5000,
            backoff_multiplier: 2.0,
        }
    }
}

/// 查询重试器
pub struct QueryRetrier {
    config: RetryConfig,
}

impl QueryRetrier {
    /// 创建新的查询重试器
    pub fn new(config: RetryConfig) -> Self {
        Self { config }
    }

    /// 使用默认配置创建查询重试器
    pub fn default() -> Self {
        Self::new(RetryConfig::default())
    }

    /// 执行带重试的查询
    pub async fn execute_with_retry<F, T, E>(&self, mut query_fn: F) -> Result<T>
    where
        F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send>>,
        E: Display + Send + 'static,
    {
        let mut last_error = None;
        
        for attempt in 1..=self.config.max_attempts {
            match query_fn().await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    warn!("Query attempt {} failed: {}", attempt, error);
                    last_error = Some(error);
                    
                    if attempt < self.config.max_attempts {
                        let delay = self.calculate_delay(attempt);
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    }
                }
            }
        }
        
        Err(anyhow!(
            "Query failed after {} attempts. Last error: {}",
            self.config.max_attempts,
            last_error.map(|e| e.to_string()).unwrap_or_else(|| "Unknown error".to_string())
        ))
    }

    /// 计算重试延迟时间
    fn calculate_delay(&self, attempt: usize) -> u64 {
        let delay = self.config.base_delay_ms as f64 
            * self.config.backoff_multiplier.powi(attempt as i32 - 1);
        
        (delay as u64).min(self.config.max_delay_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_error_conversion() {
        let query_error = QueryError::ExecutionError {
            sql: "SELECT * FROM pe".to_string(),
            message: "Table not found".to_string(),
        };
        
        let handle_error: HandleError = query_error.into();
        match handle_error {
            HandleError::QueryErr { sql, msg, .. } => {
                assert_eq!(sql, "SELECT * FROM pe");
                assert_eq!(msg, "Table not found");
            }
            _ => panic!("Unexpected error type"),
        }
    }

    #[test]
    fn test_retry_delay_calculation() {
        let retrier = QueryRetrier::default();
        
        assert_eq!(retrier.calculate_delay(1), 100);
        assert_eq!(retrier.calculate_delay(2), 200);
        assert_eq!(retrier.calculate_delay(3), 400);
    }

    #[tokio::test]
    async fn test_query_result_success() {
        let result = QueryResult::success(
            42,
            "SELECT COUNT(*) FROM pe".to_string(),
            Some(150),
        );
        
        assert_eq!(result.into_result().unwrap(), 42);
    }
}
