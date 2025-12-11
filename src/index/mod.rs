// src/index/mod.rs

/// 暴露接口
pub mod hash;

/// 引入数据结构
use crate::record::log_record::LogRecordPos;


/// 抽象索引接口，方便后续替换不同的索引数据结构
pub trait Indexer: Sync + Send {
    /// 向索引中存储 key 的位置信息
    fn put(&self, key: Vec<u8>, pos: LogRecordPos) -> bool;

    /// 获取 key 的位置信息
    fn get(&self, key: &[u8]) -> Option<LogRecordPos>;

    /// 删除 key
    fn delete(&self, key: &[u8]) -> bool;
    
    // 遍历索引 (用于 Merge/Compaction)
    // fn iterator(&self) -> Box<dyn Iterator...>; // 暂时略过
}