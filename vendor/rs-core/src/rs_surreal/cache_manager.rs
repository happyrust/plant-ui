//! 缓存管理器模块
//! 
//! 提供统一的缓存管理接口，支持多种缓存策略和自动失效机制。

use crate::types::*;
use cached::{Cached, TimedCache};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// 缓存配置
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// 缓存容量
    pub capacity: usize,
    /// 缓存过期时间（秒）
    pub ttl_seconds: u64,
    /// 是否启用缓存
    pub enabled: bool,
    /// 缓存命中率统计间隔
    pub stats_interval_seconds: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            capacity: 10000,
            ttl_seconds: 300, // 5分钟
            enabled: true,
            stats_interval_seconds: 60,
        }
    }
}

/// 缓存统计信息
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub size: usize,
    pub last_reset: Option<Instant>,
}

impl CacheStats {
    /// 计算命中率
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// 重置统计信息
    pub fn reset(&mut self) {
        self.hits = 0;
        self.misses = 0;
        self.evictions = 0;
        self.last_reset = Some(Instant::now());
    }
}

/// 通用缓存管理器
pub struct CacheManager<K, V>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    cache: Arc<RwLock<TimedCache<K, V>>>,
    stats: Arc<RwLock<CacheStats>>,
    config: CacheConfig,
}

impl<K, V> CacheManager<K, V>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// 创建新的缓存管理器
    pub fn new(config: CacheConfig) -> Self {
        let cache = TimedCache::with_lifespan_and_capacity(
            config.ttl_seconds,
            config.capacity,
        );

        Self {
            cache: Arc::new(RwLock::new(cache)),
            stats: Arc::new(RwLock::new(CacheStats::default())),
            config,
        }
    }

    /// 获取缓存值
    pub async fn get(&self, key: &K) -> Option<V> {
        if !self.config.enabled {
            return None;
        }

        let mut cache = self.cache.write().await;
        let mut stats = self.stats.write().await;

        if let Some(value) = cache.cache_get(key) {
            stats.hits += 1;
            Some(value.clone())
        } else {
            stats.misses += 1;
            None
        }
    }

    /// 设置缓存值
    pub async fn set(&self, key: K, value: V) {
        if !self.config.enabled {
            return;
        }

        let mut cache = self.cache.write().await;
        let mut stats = self.stats.write().await;

        cache.cache_set(key, value);
        stats.size = cache.cache_size();
    }

    /// 移除缓存值
    pub async fn remove(&self, key: &K) -> Option<V> {
        if !self.config.enabled {
            return None;
        }

        let mut cache = self.cache.write().await;
        let mut stats = self.stats.write().await;

        let result = cache.cache_remove(key);
        stats.size = cache.cache_size();
        result
    }

    /// 清空缓存
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        let mut stats = self.stats.write().await;

        cache.cache_clear();
        stats.evictions += stats.size as u64;
        stats.size = 0;
    }

    /// 获取缓存统计信息
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        let mut stats = self.stats.write().await;

        stats.size = cache.cache_size();
        stats.clone()
    }

    /// 重置统计信息
    pub async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.reset();
    }

    /// 检查缓存是否包含指定键
    pub async fn contains_key(&self, key: &K) -> bool {
        if !self.config.enabled {
            return false;
        }

        let cache = self.cache.read().await;
        cache.cache_get(key).is_some()
    }

    /// 获取缓存大小
    pub async fn size(&self) -> usize {
        let cache = self.cache.read().await;
        cache.cache_size()
    }
}

/// 查询缓存管理器 - 专门用于查询结果缓存
pub struct QueryCacheManager {
    /// PE 基础信息缓存
    pe_cache: CacheManager<RefnoEnum, crate::pe::SPdmsElement>,
    /// 属性映射缓存
    attr_cache: CacheManager<RefnoEnum, crate::NamedAttrMap>,
    /// 类型名称缓存
    type_cache: CacheManager<RefnoEnum, String>,
    /// 子元素缓存
    children_cache: CacheManager<RefnoEnum, Vec<RefnoEnum>>,
    /// 祖先缓存
    ancestors_cache: CacheManager<RefnoEnum, Vec<RefnoEnum>>,
}

impl QueryCacheManager {
    /// 创建新的查询缓存管理器
    pub fn new() -> Self {
        let config = CacheConfig::default();
        
        Self {
            pe_cache: CacheManager::new(config.clone()),
            attr_cache: CacheManager::new(config.clone()),
            type_cache: CacheManager::new(config.clone()),
            children_cache: CacheManager::new(config.clone()),
            ancestors_cache: CacheManager::new(config),
        }
    }

    /// 获取 PE 信息
    pub async fn get_pe(&self, refno: &RefnoEnum) -> Option<crate::pe::SPdmsElement> {
        self.pe_cache.get(refno).await
    }

    /// 设置 PE 信息
    pub async fn set_pe(&self, refno: RefnoEnum, pe: crate::pe::SPdmsElement) {
        self.pe_cache.set(refno, pe).await;
    }

    /// 获取属性映射
    pub async fn get_attributes(&self, refno: &RefnoEnum) -> Option<crate::NamedAttrMap> {
        self.attr_cache.get(refno).await
    }

    /// 设置属性映射
    pub async fn set_attributes(&self, refno: RefnoEnum, attrs: crate::NamedAttrMap) {
        self.attr_cache.set(refno, attrs).await;
    }

    /// 获取类型名称
    pub async fn get_type_name(&self, refno: &RefnoEnum) -> Option<String> {
        self.type_cache.get(refno).await
    }

    /// 设置类型名称
    pub async fn set_type_name(&self, refno: RefnoEnum, type_name: String) {
        self.type_cache.set(refno, type_name).await;
    }

    /// 获取子元素列表
    pub async fn get_children(&self, refno: &RefnoEnum) -> Option<Vec<RefnoEnum>> {
        self.children_cache.get(refno).await
    }

    /// 设置子元素列表
    pub async fn set_children(&self, refno: RefnoEnum, children: Vec<RefnoEnum>) {
        self.children_cache.set(refno, children).await;
    }

    /// 获取祖先列表
    pub async fn get_ancestors(&self, refno: &RefnoEnum) -> Option<Vec<RefnoEnum>> {
        self.ancestors_cache.get(refno).await
    }

    /// 设置祖先列表
    pub async fn set_ancestors(&self, refno: RefnoEnum, ancestors: Vec<RefnoEnum>) {
        self.ancestors_cache.set(refno, ancestors).await;
    }

    /// 清除指定 refno 的所有缓存
    pub async fn clear_refno_caches(&self, refno: &RefnoEnum) {
        self.pe_cache.remove(refno).await;
        self.attr_cache.remove(refno).await;
        self.type_cache.remove(refno).await;
        self.children_cache.remove(refno).await;
        self.ancestors_cache.remove(refno).await;
    }

    /// 清除所有缓存
    pub async fn clear_all(&self) {
        self.pe_cache.clear().await;
        self.attr_cache.clear().await;
        self.type_cache.clear().await;
        self.children_cache.clear().await;
        self.ancestors_cache.clear().await;
    }

    /// 获取所有缓存的统计信息
    pub async fn get_all_stats(&self) -> DashMap<String, CacheStats> {
        let stats = DashMap::new();
        
        stats.insert("pe_cache".to_string(), self.pe_cache.stats().await);
        stats.insert("attr_cache".to_string(), self.attr_cache.stats().await);
        stats.insert("type_cache".to_string(), self.type_cache.stats().await);
        stats.insert("children_cache".to_string(), self.children_cache.stats().await);
        stats.insert("ancestors_cache".to_string(), self.ancestors_cache.stats().await);
        
        stats
    }
}

/// 全局查询缓存实例
lazy_static::lazy_static! {
    pub static ref QUERY_CACHE: QueryCacheManager = QueryCacheManager::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_manager_basic_operations() {
        let config = CacheConfig {
            capacity: 100,
            ttl_seconds: 60,
            enabled: true,
            stats_interval_seconds: 10,
        };
        
        let cache: CacheManager<String, i32> = CacheManager::new(config);
        
        // 测试设置和获取
        cache.set("key1".to_string(), 42).await;
        assert_eq!(cache.get(&"key1".to_string()).await, Some(42));
        
        // 测试不存在的键
        assert_eq!(cache.get(&"key2".to_string()).await, None);
        
        // 测试移除
        assert_eq!(cache.remove(&"key1".to_string()).await, Some(42));
        assert_eq!(cache.get(&"key1".to_string()).await, None);
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let cache: CacheManager<String, i32> = CacheManager::new(CacheConfig::default());
        
        // 设置一些值
        cache.set("key1".to_string(), 1).await;
        cache.set("key2".to_string(), 2).await;
        
        // 测试命中
        cache.get(&"key1".to_string()).await;
        cache.get(&"key1".to_string()).await;
        
        // 测试未命中
        cache.get(&"key3".to_string()).await;
        
        let stats = cache.stats().await;
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!(stats.hit_rate() > 0.5);
    }
}
