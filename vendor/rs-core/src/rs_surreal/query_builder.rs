//! 查询构建器模块
//! 
//! 提供统一的 SurrealDB 查询构建和执行接口，减少重复代码并提高查询的一致性。

use crate::types::*;
use crate::{SurlValue, SUL_DB};
use anyhow::Result;
use surrealdb::Response;
use std::fmt::Display;

/// 查询构建器 - 用于构建和执行 SurrealDB 查询
pub struct QueryBuilder {
    sql: String,
}

impl QueryBuilder {
    /// 创建新的查询构建器
    pub fn new() -> Self {
        Self {
            sql: String::new(),
        }
    }

    /// 从 SQL 字符串创建查询构建器
    pub fn from_sql(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
        }
    }

    /// 添加 SELECT 子句
    pub fn select(mut self, fields: &str) -> Self {
        self.sql = format!("SELECT {}", fields);
        self
    }

    /// 添加 FROM 子句
    pub fn from(mut self, table: &str) -> Self {
        self.sql.push_str(&format!(" FROM {}", table));
        self
    }

    /// 添加 WHERE 子句
    pub fn where_clause(mut self, condition: &str) -> Self {
        self.sql.push_str(&format!(" WHERE {}", condition));
        self
    }

    /// 添加 LIMIT 子句
    pub fn limit(mut self, count: usize) -> Self {
        self.sql.push_str(&format!(" LIMIT {}", count));
        self
    }

    /// 添加 ORDER BY 子句
    pub fn order_by(mut self, field: &str, desc: bool) -> Self {
        let direction = if desc { "DESC" } else { "ASC" };
        self.sql.push_str(&format!(" ORDER BY {} {}", field, direction));
        self
    }

    /// 获取构建的 SQL 字符串
    pub fn build(&self) -> &str {
        &self.sql
    }

    /// 执行查询并返回响应
    pub async fn execute(&self) -> Result<Response> {
        let response = SUL_DB.query(&self.sql).await?;
        Ok(response)
    }

    /// 执行查询并获取单个结果
    pub async fn fetch_one<T>(&self) -> Result<Option<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut response = self.execute().await?;
        let result: Option<T> = response.take(0)?;
        Ok(result)
    }

    /// 执行查询并获取多个结果
    pub async fn fetch_all<T>(&self) -> Result<Vec<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut response = self.execute().await?;
        let results: Vec<T> = response.take(0)?;
        Ok(results)
    }

    /// 执行查询并获取 SurrealDB Value
    pub async fn fetch_value(&self) -> Result<SurlValue> {
        let mut response = self.execute().await?;
        let value: SurlValue = response.take(0)?;
        Ok(value)
    }
}

/// 专门用于构建 PE (Plant Element) 查询的构建器
pub struct PeQueryBuilder {
    refno: RefnoEnum,
}

impl PeQueryBuilder {
    /// 创建新的 PE 查询构建器
    pub fn new(refno: RefnoEnum) -> Self {
        Self { refno }
    }

    /// 构建基础 PE 查询
    pub fn basic_query(&self) -> QueryBuilder {
        QueryBuilder::from_sql(format!(
            "SELECT * OMIT id FROM ONLY {} LIMIT 1",
            self.refno.to_pe_key()
        ))
    }

    /// 构建属性查询
    pub fn attributes_query(&self) -> QueryBuilder {
        QueryBuilder::from_sql(format!(
            "(SELECT * FROM {}.refno)[0]",
            self.refno.to_pe_key()
        ))
    }

    /// 构建子元素查询
    pub fn children_query(&self) -> QueryBuilder {
        QueryBuilder::from_sql(format!(
            "SELECT value in FROM {}<-pe_owner WHERE in.id!=none AND record::exists(in.id) AND !in.deleted",
            self.refno.to_pe_key()
        ))
    }

    /// 构建祖先查询
    pub fn ancestors_query(&self) -> QueryBuilder {
        QueryBuilder::from_sql(format!(
            "RETURN fn::ancestor({}).refno",
            self.refno.to_pe_key()
        ))
    }

    /// 构建类型查询
    pub fn type_query(&self) -> QueryBuilder {
        QueryBuilder::from_sql(format!(
            "SELECT value noun FROM ONLY {} LIMIT 1",
            self.refno.to_pe_key()
        ))
    }
}

/// 批量查询构建器
pub struct BatchQueryBuilder {
    refnos: Vec<RefnoEnum>,
}

impl BatchQueryBuilder {
    /// 创建新的批量查询构建器
    pub fn new(refnos: Vec<RefnoEnum>) -> Self {
        Self { refnos }
    }

    /// 构建批量类型查询
    pub fn types_query(&self) -> QueryBuilder {
        let pe_keys = self.refnos.iter().map(|x| x.to_pe_key()).collect::<Vec<_>>().join(",");
        QueryBuilder::from_sql(format!(
            "SELECT value noun FROM [{}]",
            pe_keys
        ))
    }

    /// 构建批量全名查询
    pub fn full_names_query(&self) -> QueryBuilder {
        let pe_keys = self.refnos.iter().map(|x| x.to_pe_key()).collect::<Vec<_>>().join(",");
        QueryBuilder::from_sql(format!(
            "SELECT value fn::default_full_name(id) FROM [{}]",
            pe_keys
        ))
    }

    /// 构建批量子元素查询
    pub fn all_children_query(&self) -> QueryBuilder {
        let pe_keys = self.refnos.iter().map(|x| x.to_pe_key()).collect::<Vec<_>>().join(",");
        QueryBuilder::from_sql(format!(
            "array::flatten(SELECT value in FROM [{}]<-pe_owner WHERE record::exists(in.id) AND !in.deleted)",
            pe_keys
        ))
    }
}

/// 函数查询构建器 - 用于调用 SurrealDB 函数
pub struct FunctionQueryBuilder;

impl FunctionQueryBuilder {
    /// 构建默认名称查询
    pub fn default_name(refno: RefnoEnum) -> QueryBuilder {
        QueryBuilder::from_sql(format!(
            "RETURN fn::default_name({})",
            refno.to_pe_key()
        ))
    }

    /// 构建默认全名查询
    pub fn default_full_name(refno: RefnoEnum) -> QueryBuilder {
        QueryBuilder::from_sql(format!(
            "RETURN fn::default_full_name({})",
            refno.to_pe_key()
        ))
    }

    /// 构建祖先类型查询
    pub fn find_ancestor_type(refno: RefnoEnum, ancestor_type: &str) -> QueryBuilder {
        QueryBuilder::from_sql(format!(
            "RETURN fn::find_ancestor_type({}, '{}')",
            refno.to_pe_key(),
            ancestor_type
        ))
    }

    /// 构建会话日期查询
    pub fn session_date(refno: RefnoEnum) -> QueryBuilder {
        QueryBuilder::from_sql(format!(
            "RETURN fn::ses_date({})",
            refno.to_pe_key()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_builder() {
        let query = QueryBuilder::new()
            .select("*")
            .from("pe")
            .where_clause("noun = 'PIPE'")
            .limit(10)
            .order_by("name", false);
        
        assert_eq!(
            query.build(),
            "SELECT * FROM pe WHERE noun = 'PIPE' LIMIT 10 ORDER BY name ASC"
        );
    }

    #[test]
    fn test_pe_query_builder() {
        let refno = RefnoEnum::from("12345_67890");
        let builder = PeQueryBuilder::new(refno);
        let query = builder.basic_query();
        
        assert!(query.build().contains("SELECT * OMIT id FROM ONLY"));
        assert!(query.build().contains("pe:['12345_67890']"));
    }
}
