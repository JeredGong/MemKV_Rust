/// 暴露Record下的接口给lib

/// Record的实现-数据结构/序列化/反序列化
pub mod log_record;

/// 文件抽象的实现：ID/句柄/游标
pub mod data_file;

/// 跨平台地实现无游标读
pub mod dbio;

/// 日志文件的合并逻辑
pub mod merge;