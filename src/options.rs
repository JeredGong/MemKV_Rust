// src/options.rs

use std::path::PathBuf;

/// 数据库配置项
#[derive(Clone, Debug)]
pub struct Options {
    /// 数据库目录路径
    pub dir_path: PathBuf,
    
    /// 数据文件最大阈值，超过则触发 Rotate (例如 256MB)
    pub data_file_size: u64,
    
    /// 每次写入是否持久化 (Sync to disk)
    /// true: 数据安全性最高，但性能较低
    /// false: 依赖操作系统缓存，性能高
    pub sync_writes: bool,
    
    /// 索引类型 (目前默认 HashMap)
    pub index_type: IndexType,

    /// 如果数据库目录不存在，是否自动创建
    /// true: 自动创建目录
    /// false: 如果目录不存在则报错
    pub create_if_missing: bool,

    /// 碎片率阈值 (0.0 ~ 1.0)
    /// 当 (无效数据 / 文件总大小) 超过此比例时，触发该文件的合并。
    /// 推荐值: 0.5 (即 50% 都是垃圾时才合并)
    pub compaction_file_threshold: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]

/// 索引类型
pub enum IndexType {
    /// BTree 索引 (后续可扩展)
    BTree,
    /// HashMap 索引 (Bitcask 默认)
    HashMap,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            dir_path: std::env::current_dir().unwrap(),
            data_file_size: 1 * 1024 * 1024, // 1MB，便于触发轮转与合并
            sync_writes: false,
            index_type: IndexType::HashMap,
            create_if_missing: true, // 默认允许自动创建
            compaction_file_threshold:0.5,
        }
    }
}
