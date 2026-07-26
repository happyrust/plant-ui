//! 层次结构查询模块
//! 
//! 提供元素层次结构相关的查询功能，包括祖先查询、子元素查询、兄弟元素查询等。

use crate::pdms_types::EleTreeNode;
use crate::rs_surreal::cache_manager::QUERY_CACHE;
use crate::rs_surreal::error_handler::{QueryError, QueryErrorHandler};
use crate::rs_surreal::query_builder::{BatchQueryBuilder, FunctionQueryBuilder, PeQueryBuilder, QueryBuilder};
use crate::types::*;
use anyhow::Result;
use cached::proc_macro::cached;
use indexmap::IndexMap;
use itertools::Itertools;
use std::collections::HashMap;
use std::time::Instant;

/// 层次结构查询服务
pub struct HierarchyQueryService;

impl HierarchyQueryService {
    /// 查询祖先节点列表
    /// 
    /// # 参数
    /// * `refno` - 要查询的参考号
    /// 
    /// # 返回值
    /// * `Result<Vec<RefnoEnum>>` - 祖先节点的 refno 列表
    /// 
    /// # 错误
    /// * 如果查询失败会返回错误
    pub async fn query_ancestor_refnos(refno: RefnoEnum) -> Result<Vec<RefnoEnum>> {
        let start_time = Instant::now();
        
        // 先检查缓存
        if let Some(cached_ancestors) = QUERY_CACHE.get_ancestors(&refno).await {
            return Ok(cached_ancestors);
        }

        // 构建查询
        let query = PeQueryBuilder::new(refno).ancestors_query();
        let sql = query.build().to_string();

        // 执行查询
        match query.fetch_all::<RefnoEnum>().await {
            Ok(ancestors) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);
                QueryErrorHandler::log_query_results(&sql, ancestors.len());

                // 缓存结果
                QUERY_CACHE.set_ancestors(refno, ancestors.clone()).await;

                Ok(ancestors)
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }

    /// 查询指定类型的第一个祖先节点
    /// 
    /// # 参数
    /// * `refno` - 要查询的参考号
    /// * `ancestor_type` - 要查询的祖先节点类型
    /// 
    /// # 返回值
    /// * `Result<Option<RefnoEnum>>` - 如果找到则返回对应的祖先节点 refno，否则返回 None
    /// 
    /// # 错误
    /// * 如果查询失败会返回错误
    pub async fn query_ancestor_of_type(
        refno: RefnoEnum,
        ancestor_type: String,
    ) -> Result<Option<RefnoEnum>> {
        let start_time = Instant::now();
        let query = FunctionQueryBuilder::find_ancestor_type(refno, &ancestor_type);
        let sql = query.build().to_string();

        match query.fetch_one::<RefnoEnum>().await {
            Ok(ancestor) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);
                Ok(ancestor)
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }

    /// 获取指定 refno 的所有祖先节点的类型名称
    /// 
    /// # 参数
    /// * `refno` - 要查询的参考号
    /// 
    /// # 返回值
    /// * `Result<Vec<String>>` - 祖先节点的类型名称列表
    /// 
    /// # 错误
    /// * 如果查询失败会返回错误
    pub async fn get_ancestor_types(refno: RefnoEnum) -> Result<Vec<String>> {
        let start_time = Instant::now();
        let sql = format!("RETURN fn::ancestor({}).noun", refno.to_pe_key());
        let query = QueryBuilder::from_sql(&sql);

        match query.fetch_all::<String>().await {
            Ok(types) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);
                QueryErrorHandler::log_query_results(&sql, types.len());
                Ok(types)
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }

    /// 获取子元素的 refno 列表
    /// 
    /// # 参数
    /// * `refno` - 父元素的参考号
    /// 
    /// # 返回值
    /// * `Result<Vec<RefnoEnum>>` - 子元素的 refno 列表
    /// 
    /// # 错误
    /// * 如果查询失败会返回错误
    pub async fn get_children_refnos(refno: RefnoEnum) -> Result<Vec<RefnoEnum>> {
        let start_time = Instant::now();
        
        // 先检查缓存
        if let Some(cached_children) = QUERY_CACHE.get_children(&refno).await {
            return Ok(cached_children);
        }

        // 构建查询
        let query = if refno.is_latest() {
            PeQueryBuilder::new(refno).children_query()
        } else {
            // 历史版本查询
            let sql = format!(
                r#" 
                LET $dt=<datetime>fn::ses_date({0}); 
                SELECT value fn::find_pe_by_datetime(in, $dt) FROM fn::newest_pe({0})<-pe_owner 
                    WHERE in.id!=none AND record::exists(in.id) AND (!in.deleted OR <datetime>fn::ses_date(in.id)>$dt)
                "#,
                refno.to_pe_key(),
            );
            QueryBuilder::from_sql(sql)
        };

        let sql = query.build().to_string();

        // 执行查询
        match query.fetch_all::<RefnoEnum>().await {
            Ok(children) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);
                QueryErrorHandler::log_query_results(&sql, children.len());

                // 缓存结果
                QUERY_CACHE.set_children(refno, children.clone()).await;

                Ok(children)
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }

    /// 获取兄弟元素列表
    /// 
    /// # 参数
    /// * `refno` - 要查询的参考号
    /// 
    /// # 返回值
    /// * `Result<Vec<RefnoEnum>>` - 兄弟元素的 refno 列表
    /// 
    /// # 错误
    /// * 如果查询失败会返回错误
    pub async fn get_siblings(refno: RefnoEnum) -> Result<Vec<RefnoEnum>> {
        let start_time = Instant::now();
        let sql = format!("SELECT value in FROM {}<-pe_owner", refno.to_pe_key());
        let query = QueryBuilder::from_sql(&sql);

        match query.fetch_all::<RefnoEnum>().await {
            Ok(siblings) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);
                QueryErrorHandler::log_query_results(&sql, siblings.len());
                Ok(siblings)
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }

    /// 获取下一个或上一个兄弟元素
    /// 
    /// # 参数
    /// * `refno` - 当前元素的参考号
    /// * `next` - true 获取下一个，false 获取上一个
    /// 
    /// # 返回值
    /// * `Result<RefnoEnum>` - 兄弟元素的 refno，如果不存在则返回默认值
    /// 
    /// # 错误
    /// * 如果查询失败会返回错误
    pub async fn get_next_prev(refno: RefnoEnum, next: bool) -> Result<RefnoEnum> {
        let siblings = Self::get_siblings(refno).await?;
        let pos = siblings
            .iter()
            .position(|x| *x == refno)
            .unwrap_or_default();

        if next {
            Ok(siblings.get(pos + 1).cloned().unwrap_or_default())
        } else {
            if pos == 0 {
                return Ok(Default::default());
            }
            Ok(siblings.get(pos - 1).cloned().unwrap_or_default())
        }
    }

    /// 批量获取多个 refno 的所有子元素
    /// 
    /// # 参数
    /// * `refnos` - 父元素的参考号列表
    /// 
    /// # 返回值
    /// * `Result<Vec<RefnoEnum>>` - 所有子元素的 refno 列表
    /// 
    /// # 错误
    /// * 如果查询失败会返回错误
    pub async fn query_multi_children_refnos(refnos: &[RefnoEnum]) -> Result<Vec<RefnoEnum>> {
        let mut final_refnos = Vec::new();
        
        for &refno in refnos {
            match Self::get_children_refnos(refno).await {
                Ok(children) => {
                    final_refnos.extend(children);
                }
                Err(e) => {
                    eprintln!("获取子参考号时出错: refno={:?}, 错误: {:?}", refno, e);
                    return Err(e);
                }
            }
        }
        
        Ok(final_refnos)
    }

    /// 获取子元素的树节点信息
    /// 
    /// # 参数
    /// * `refno` - 父元素的参考号
    /// 
    /// # 返回值
    /// * `Result<Vec<EleTreeNode>>` - 子元素的树节点信息列表
    /// 
    /// # 错误
    /// * 如果查询失败会返回错误
    pub async fn get_children_ele_nodes(refno: RefnoEnum) -> Result<Vec<EleTreeNode>> {
        let start_time = Instant::now();
        let sql = format!(
            r#"
            SELECT in.refno as refno, in.noun as noun, in.name as name, in.owner as owner, 
                   record::id(in->pe_owner.id[0])[1] as order,
                   in.op?:0 as op,
                   array::len((SELECT value refnos FROM ONLY type::thing("his_pe", record::id(in.refno)))?:[]) as mod_cnt,
                   array::len(in<-pe_owner) as children_count,
                   in.status_code as status_code
            FROM {}<-pe_owner WHERE in.id!=none AND record::exists(in.id) AND !in.deleted
            "#,
            refno.to_pe_key()
        );
        
        let query = QueryBuilder::from_sql(&sql);

        match query.fetch_all::<EleTreeNode>().await {
            Ok(mut nodes) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);
                QueryErrorHandler::log_query_results(&sql, nodes.len());

                // 检查名称，如果没有给名字的，需要给上默认值
                let mut hashmap: HashMap<&str, i32> = HashMap::new();
                for node in &mut nodes {
                    if node.name.is_empty() {
                        let mut n = 1;
                        if let Some(k) = hashmap.get_mut(&node.noun.as_str()) {
                            *k += 1;
                            n = *k;
                        } else {
                            hashmap.insert(node.noun.as_str(), 1);
                        }
                        node.name = format!("{} {}", node.noun.as_str(), n);
                    }
                }

                Ok(nodes)
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }

    /// 在父节点下的索引，noun 有值时按照 noun 过滤
    /// 
    /// # 参数
    /// * `parent` - 父节点的参考号
    /// * `refno` - 子节点的参考号
    /// * `noun` - 可选的类型过滤条件
    /// 
    /// # 返回值
    /// * `Result<Option<u32>>` - 在父节点下的索引位置
    /// 
    /// # 错误
    /// * 如果查询失败会返回错误
    pub async fn get_index_by_noun_in_parent(
        parent: RefnoEnum,
        refno: RefnoEnum,
        noun: Option<&str>,
    ) -> Result<Option<u32>> {
        let start_time = Instant::now();
        let sql = format!(
            r#"
            array::find_index((SELECT value in.id FROM {}<-pe_owner {}), {})
            "#,
            parent.to_pe_key(),
            if let Some(noun) = noun {
                format!("WHERE in.noun='{}'", noun)
            } else {
                "".to_owned()
            },
            refno.to_pe_key()
        );

        let query = QueryBuilder::from_sql(&sql);

        match query.fetch_one::<u32>().await {
            Ok(index) => {
                let execution_time = start_time.elapsed().as_millis() as u64;
                QueryErrorHandler::log_query_execution(&sql, execution_time);
                Ok(index)
            }
            Err(error) => {
                let query_error = QueryErrorHandler::handle_execution_error(&sql, error);
                Err(query_error.into())
            }
        }
    }
}

/// 缓存版本的层次结构查询函数（保持向后兼容）
#[cached(result = true)]
pub async fn query_ancestor_refnos(refno: RefnoEnum) -> anyhow::Result<Vec<RefnoEnum>> {
    HierarchyQueryService::query_ancestor_refnos(refno).await
}

#[cached(result = true)]
pub async fn query_ancestor_of_type(
    refno: RefnoEnum,
    ancestor_type: String,
) -> anyhow::Result<Option<RefnoEnum>> {
    HierarchyQueryService::query_ancestor_of_type(refno, ancestor_type).await
}

#[cached(result = true)]
pub async fn get_ancestor_types(refno: RefnoEnum) -> anyhow::Result<Vec<String>> {
    HierarchyQueryService::get_ancestor_types(refno).await
}

#[cached(result = true)]
pub async fn get_children_refnos(refno: RefnoEnum) -> anyhow::Result<Vec<RefnoEnum>> {
    HierarchyQueryService::get_children_refnos(refno).await
}

#[cached(result = true)]
pub async fn get_siblings(refno: RefnoEnum) -> anyhow::Result<Vec<RefnoEnum>> {
    HierarchyQueryService::get_siblings(refno).await
}

#[cached(result = true)]
pub async fn get_next_prev(refno: RefnoEnum, next: bool) -> anyhow::Result<RefnoEnum> {
    HierarchyQueryService::get_next_prev(refno, next).await
}

pub async fn query_multi_children_refnos(refnos: &[RefnoEnum]) -> anyhow::Result<Vec<RefnoEnum>> {
    HierarchyQueryService::query_multi_children_refnos(refnos).await
}

pub async fn get_children_ele_nodes(refno: RefnoEnum) -> anyhow::Result<Vec<EleTreeNode>> {
    HierarchyQueryService::get_children_ele_nodes(refno).await
}

pub async fn get_index_by_noun_in_parent(
    parent: RefnoEnum,
    refno: RefnoEnum,
    noun: Option<&str>,
) -> anyhow::Result<Option<u32>> {
    HierarchyQueryService::get_index_by_noun_in_parent(parent, refno, noun).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hierarchy_query_service() {
        let refno = RefnoEnum::from("12345_67890");
        
        // 测试祖先查询的错误处理
        match HierarchyQueryService::query_ancestor_refnos(refno).await {
            Ok(_) => {
                // 查询成功
            }
            Err(_) => {
                // 预期的错误，因为没有实际的数据库连接
            }
        }
    }
}
