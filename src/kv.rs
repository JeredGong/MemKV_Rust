use crate::engine::KVEngine;
use crate::options::Options;
use std::io;

/// `KvStore` 是 Bitcask 存储引擎的公共 API 封装。
///
/// 通过底层的 `Engine` 模块处理
pub struct KvStore {
    engine: KVEngine,
}

impl KvStore {
    /// 打开一个 KvStore。
    ///
    /// 这将加载指定目录下的数据文件，并重建内存索引。
    /// 
    /// Example:
    ///
    /// ```rust
    /// use kvs::KvStore;
    /// let store = KvStore::open("./db_dir").unwrap();
    /// ```
    pub fn open(path: impl Into<std::path::PathBuf>) -> io::Result<KvStore> {
        let path = path.into();
        
        // 构造默认配置
        let mut opts = Options::default();
        opts.dir_path = path;
        
        // 初始化底层引擎
        let engine = KVEngine::open(opts)?;
        
        Ok(KvStore { engine })
    }

    /// 设置 key 对应的 value。
    ///
    /// 数据会被持久化到磁盘。
    pub fn set(&self, key: String, value: String) -> io::Result<()> {
        // 将 String 转换为 Bytes 并传递给引擎
        // 这里使用了 bytes::Bytes，它是轻量级的引用计数切片
        self.engine.put(key.into(), value.into())
    }

    /// 获取 key 对应的 value。
    ///
    /// 如果 key 不存在，返回 Ok(None)。
    pub fn get(&self, key: String) -> io::Result<Option<String>> {
        match self.engine.get(key.into()) {
            Ok(Some(val_bytes)) => {
                // 将读取到的 Bytes 转换回 String
                // 假设存储的都是合法的 UTF-8 字符串
                let value = String::from_utf8(val_bytes.to_vec())
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                Ok(Some(value))
            },
            Ok(None) => { Ok(None)},
            Err(e) => {
                // 如果底层引擎返回 "KeyNotFound" 类型的错误，我们将其转换为 None 返回
                // 这样符合 Rust 集合库的习惯
                if e.kind() == io::ErrorKind::NotFound {
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        }
    }

    /// 删除指定的 key。
    ///
    /// 这实际上是写入一条墓碑记录（Tombstone）。
    pub fn remove(&self, key: String) -> io::Result<()> {
        self.engine.delete(key.into())
    }
}