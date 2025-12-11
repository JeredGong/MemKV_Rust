use std::collections::HashMap;
use crate::record::data_file::DataFile;
use crate::record::log_record::{LogRecord, LogRecordPos, LogRecordType};
use std::path::PathBuf;
use std::io;

/// 合并日志的上下文结构体
pub struct MergeContext {
    /// 已经写好的临时文件列表
    finished_files: Vec<DataFile>,
    /// 当前正在写的文件
    current_file: DataFile,
    /// 记录重写后的 Key -> Pos 映射
    new_positions: HashMap<Vec<u8>, LogRecordPos>,
    /// 当前正在写的临时文件 ID (从 0 开始递增)
    current_merge_file_id: u32,
    /// 基础路径
    merge_path: PathBuf,
    /// 单个文件大小限制
    file_size_limit: u64,
}

impl MergeContext{
    /// 新建一个MergeContext上下文，用于管理Merge过程中的文件
    pub fn new(merge_path: PathBuf, file_size_limit: u64) -> io::Result<Self> {
        let current_file = DataFile::new(&merge_path, 0)?;
        Ok(Self {
            finished_files: Vec::new(),
            current_file,
            new_positions: HashMap::new(),
            current_merge_file_id: 0,
            merge_path,
            file_size_limit,
        })
    }

    /// 写入一条记录，如果文件满了自动切分
    pub fn append(&mut self, key: Vec<u8>, value: &[u8], rec_type: LogRecordType) -> io::Result<()> {
        let record = LogRecord { key: key.clone(), value: value.to_vec(), rec_type };
        let (enc_data, size) = record.encode();

        // 检查是否需要切分新文件
        if self.current_file.get_write_offset() + size as u64 > self.file_size_limit {
            // 1. 持久化当前文件
            self.current_file.sync()?;

            // 2. 将当前文件移入 finished 列表 

            // 创建新文件
            let new_file_id = self.current_merge_file_id + 1;
            let new_file = DataFile::new(&self.merge_path, new_file_id)?;
            // 交换
            let old_file = std::mem::replace(&mut self.current_file, new_file);
            self.finished_files.push(old_file);
            
            // 最后自增，确保ID安全
            self.current_merge_file_id = new_file_id;
        }

        // 写入数据
        let offset = self.current_file.write(&enc_data)?;
        
        // 记录位置 (使用临时的 merge_file_id)
        let pos = LogRecordPos {
            file_id: self.current_merge_file_id,
            offset,
            size: size as u32,
        };
        self.new_positions.insert(key, pos);
        
        Ok(())
    }

    /// 完成所有写入
    pub fn finish(mut self) -> io::Result<(Vec<DataFile>, HashMap<Vec<u8>, LogRecordPos>)> {
        self.current_file.sync()?;
        self.finished_files.push(self.current_file);
        Ok((self.finished_files, self.new_positions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn split_when_limit_exceeded() -> io::Result<()> {
        let dir = tempdir()?;
        // 25 字节的上限，小于两条记录之和，第二条会触发切分
        let limit = 25u64;
        let mut ctx = MergeContext::new(dir.path().to_path_buf(), limit)?;

        // 第一条写入，不触发切分
        ctx.append(b"k1".to_vec(), b"v1", LogRecordType::Normal)?;
        assert_eq!(ctx.current_merge_file_id, 0);
        assert_eq!(ctx.finished_files.len(), 0);

        // 第二条写入前超限，触发切分，生成 id=1 文件
        ctx.append(b"k2".to_vec(), b"v2", LogRecordType::Normal)?;
        assert_eq!(ctx.current_merge_file_id, 1);
        assert_eq!(ctx.finished_files.len(), 1);

        let (files, positions) = ctx.finish()?;
        assert_eq!(files.len(), 2);
        assert_eq!(positions.len(), 2);

        // 便于按 file_id 查找
        let mut file_map = HashMap::new();
        for f in files {
            file_map.insert(f.get_file_id(), f);
        }

        // k1 应在第一个文件
        let pos1 = positions.get(&b"k1".to_vec()).unwrap();
        assert_eq!(pos1.file_id, 0);
        let rec1 = file_map
            .get(&0)
            .unwrap()
            .read_log_record(pos1.offset)?;
        assert_eq!(rec1.key, b"k1".to_vec());
        assert_eq!(rec1.value, b"v1".to_vec());

        // k2 应在第二个文件
        let pos2 = positions.get(&b"k2".to_vec()).unwrap();
        assert_eq!(pos2.file_id, 1);
        let rec2 = file_map
            .get(&1)
            .unwrap()
            .read_log_record(pos2.offset)?;
        assert_eq!(rec2.key, b"k2".to_vec());
        assert_eq!(rec2.value, b"v2".to_vec());

        Ok(())
    }

    #[test]
    fn offset_resets_after_split() -> io::Result<()> {
        let dir = tempdir()?;
        let limit = 24u64; // 让第二条触发切分
        let mut ctx = MergeContext::new(dir.path().to_path_buf(), limit)?;

        ctx.append(b"a".to_vec(), b"1", LogRecordType::Normal)?;
        ctx.append(b"b".to_vec(), b"2", LogRecordType::Normal)?; // 触发切分
        let (_files, positions) = ctx.finish()?;

        let pos_a = positions.get(&b"a".to_vec()).unwrap();
        let pos_b = positions.get(&b"b".to_vec()).unwrap();

        // 第一条在旧文件的起始位置，第二条在新文件的起始位置
        assert_eq!(pos_a.offset, 0);
        assert_eq!(pos_b.offset, 0);
        assert_ne!(pos_a.file_id, pos_b.file_id);

        Ok(())
    }
}
