use crate::record::log_record::LogRecordPos;
use crate::index::Indexer;
use parking_lot::RwLock;
use std::collections::HashMap;

/// 基于 HashMap 的索引实现
pub struct HashIndexer {
    // 使用 RwLock 包装 HashMap，实现细粒度的并发控制
    container: RwLock<HashMap<Vec<u8>, LogRecordPos>>,
}

impl HashIndexer {
    /// 创建一个新的 HashIndexer
    pub fn new() -> Self {
        Self {
            container: RwLock::new(HashMap::new()),
        }
    }
}

impl Indexer for HashIndexer {
    fn put(&self, key: Vec<u8>, pos: LogRecordPos) -> bool {
        let mut write_guard = self.container.write();
        write_guard.insert(key, pos).is_some()
    }

    fn get(&self, key: &[u8]) -> Option<LogRecordPos> {
        let read_guard = self.container.read();
        // 这里的 cloned 会复制 LogRecordPos (它是 Copy 类型，开销极小)
        read_guard.get(key).cloned()
    }

    fn delete(&self, key: &[u8]) -> bool {
        let mut write_guard = self.container.write();
        write_guard.remove(key).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_indexer_put_get_delete() {
        let indexer = HashIndexer::new();
        let key = vec![1u8, 2, 3];
        let pos = LogRecordPos {
            file_id: 1,
            offset: 100,
            size: 20, // 假设加上 size 字段
        };

        // 1. Test Put
        let res1 = indexer.put(key.clone(), pos);
        assert!(!res1); // 新 key，返回 false

        let res2 = indexer.put(key.clone(), pos);
        assert!(res2); // 更新 key，返回 true

        // 2. Test Get
        let val = indexer.get(&key);
        assert!(val.is_some());
        assert_eq!(val.unwrap(), pos);

        // 3. Test Delete
        let del_res = indexer.delete(&key);
        assert!(del_res);

        let val_after_del = indexer.get(&key);
        assert!(val_after_del.is_none());
    }
}