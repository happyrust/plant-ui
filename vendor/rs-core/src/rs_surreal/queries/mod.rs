//! 查询模块
//! 
//! 这个模块包含了所有重构后的查询功能，按功能分类组织：
//! 
//! - `basic`: 基础查询功能
//! - `hierarchy`: 层次结构查询功能  
//! - `attributes`: 属性查询功能
//! - `batch`: 批量查询功能
//! - `history`: 历史数据查询功能

pub mod basic;
pub mod hierarchy;
pub mod attributes;
pub mod batch;

// 重新导出主要的查询服务
pub use basic::BasicQueryService;
pub use hierarchy::HierarchyQueryService;
pub use attributes::AttributeQueryService;
pub use batch::BatchQueryService;

// 重新导出向后兼容的函数
pub use basic::{
    get_pe, get_type_name, get_default_name, get_refno_by_name, 
    get_type_names, get_default_full_name
};

pub use hierarchy::{
    query_ancestor_refnos, query_ancestor_of_type, get_ancestor_types,
    get_children_refnos, get_siblings, get_next_prev, query_multi_children_refnos,
    get_children_ele_nodes, get_index_by_noun_in_parent
};

pub use attributes::{
    get_named_attmap, get_named_attmap_with_uda, get_ui_named_attmap,
    get_ancestor_attmaps, get_children_named_attmaps, query_single_by_paths
};

pub use batch::{
    query_full_names, query_full_names_map, query_children_full_names_map,
    query_data_with_refno_to_name, query_multiple_refnos_to_names,
    query_refnos_to_names_list, get_all_children_refnos, query_types,
    query_filter_children, query_filter_children_atts
};

/// 查询模块的统一接口
pub struct QueryService;

impl QueryService {
    /// 基础查询服务
    pub fn basic() -> &'static BasicQueryService {
        &BasicQueryService
    }

    /// 层次结构查询服务
    pub fn hierarchy() -> &'static HierarchyQueryService {
        &HierarchyQueryService
    }

    /// 属性查询服务
    pub fn attributes() -> &'static AttributeQueryService {
        &AttributeQueryService
    }

    /// 批量查询服务
    pub fn batch() -> &'static BatchQueryService {
        &BatchQueryService
    }
}

/// 清除所有缓存
pub async fn clear_all_caches(refno: crate::types::RefnoEnum) {
    use crate::rs_surreal::cache_manager::QUERY_CACHE;
    use cached::Cached;
    
    // 清除新的缓存系统
    QUERY_CACHE.clear_refno_caches(&refno).await;
    
    // 清除旧的缓存系统（保持向后兼容）
    crate::GET_WORLD_TRANSFORM.lock().await.cache_clear();
    crate::GET_WORLD_MAT4.lock().await.cache_clear();
    crate::graph::QUERY_DEEP_CHILDREN_REFNOS.lock().await.cache_remove(&refno);
    
    // 清除各个函数的缓存
    basic::GET_PE.lock().await.cache_remove(&refno);
    basic::GET_TYPE_NAME.lock().await.cache_remove(&refno);
    basic::GET_DEFAULT_FULL_NAME.lock().await.cache_remove(&refno);
    
    hierarchy::QUERY_ANCESTOR_REFNOS.lock().await.cache_remove(&refno);
    hierarchy::GET_CHILDREN_REFNOS.lock().await.cache_remove(&refno);
    hierarchy::GET_SIBLINGS.lock().await.cache_remove(&refno);
    
    attributes::GET_NAMED_ATTMAP.lock().await.cache_remove(&refno);
    attributes::GET_NAMED_ATTMAP_WITH_UDA.lock().await.cache_remove(&refno);
    attributes::GET_CHILDREN_NAMED_ATTMAPS.lock().await.cache_remove(&refno);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RefnoEnum;

    #[tokio::test]
    async fn test_query_service_interface() {
        let refno = RefnoEnum::from("12345_67890");
        
        // 测试统一接口
        let basic_service = QueryService::basic();
        let hierarchy_service = QueryService::hierarchy();
        let attributes_service = QueryService::attributes();
        let batch_service = QueryService::batch();
        
        // 这些服务应该是可用的
        assert!(!std::ptr::eq(basic_service, hierarchy_service as *const _ as *const BasicQueryService));
    }

    #[tokio::test]
    async fn test_clear_caches() {
        let refno = RefnoEnum::from("12345_67890");
        
        // 测试缓存清除功能
        clear_all_caches(refno).await;
        
        // 这个测试主要确保函数能正常调用而不出错
    }
}
