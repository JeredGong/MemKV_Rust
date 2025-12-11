use crate::record::data_file::DataFile;
use crate::record::log_record::{LogRecord, LogRecordPos, LogRecordType};
use crate::index::{self, Indexer};
use crate::options::Options;
use bytes::Bytes;
use parking_lot::RwLock;
use crate::record::merge::MergeContext;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Error, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
const MERGE_DIR_NAME: &str = "merge";
const MERGE_FINISHED_FILE_NAME: &str = "MERGE_FINISHED";
use std::collections::HashSet;
use super::KvsEngine;
/// Engine 数据结构
/// 
pub struct KVEngine {
    options: Options,
    
    /// 当前活跃文件：用于写入
    /// 使用 RwLock 是为了允许 "多读一写"
    active_file: Arc<RwLock<DataFile>>,
    
    /// 旧文件集合：只读
    /// Map: FileID -> DataFile
    older_files: Arc<RwLock<HashMap<u32, DataFile>>>,
    
    /// 内存索引：将 Key 映射到磁盘位置
    index: Box<dyn Indexer>,
    
    /// 记录所有文件 ID，用于加载索引和合并
    file_ids: RwLock<Vec<u32>>,

    /// 记录每个文件的无效字节数 (FileID -> Invalid Bytes)
    file_reclaim_size: RwLock<HashMap<u32, u64>>,

    /// 标记是否有合并正在进行，CAS 防止并发合并
    merge_running: AtomicBool,
}

impl KVEngine {
    /// 打开文件
    pub fn open(opts: Options) -> io::Result<Self> {
        // 1. 校验配置
        if !opts.dir_path.exists() {
            if !opts.create_if_missing {
                 return Err(Error::new(ErrorKind::NotFound, "database dir not found"));
            }
            fs::create_dir_all(&opts.dir_path)?;
        }

        // 2. 加载数据文件
        let mut file_ids = load_data_file_ids(&opts.dir_path)?;
        // 如果目录为空，初始化一个 ID 为 0 的文件
        if file_ids.is_empty() {
            file_ids.push(0);
        }

        // 3. 打开所有文件
        let mut older_files = HashMap::new();
        let mut active_file = None;

        // 这里的逻辑是：ID 最大的那个是 Active File，其余是 Older Files
        let active_file_id = *file_ids.last().unwrap();

        for &id in file_ids.iter() {
            let file = DataFile::new(&opts.dir_path, id)?;
            if id == active_file_id {
                active_file = Some(RwLock::new(file));
            } else {
                older_files.insert(id, file);
            }
        }
        let file_ids = RwLock::new(file_ids);
        // 4. 初始化索引
        // 这里根据配置选择具体的索引实现，目前我们只有 HashIndexer
        let index: Box<dyn Indexer> = Box::new(index::hash::HashIndexer::new());
        let file_reclaim_size = RwLock::new(HashMap::new());
        // 5. 构造 Engine 实例
        let engine = Self {
            options: opts,
            active_file: Arc::new(active_file.unwrap()),
            older_files: Arc::new(RwLock::new(older_files)),
            index,
            file_ids,
            file_reclaim_size,
            merge_running: AtomicBool::new(false),
        };

        // 6. 从文件加载索引 (Crash Recovery)
        engine.load_index_from_files()?;

        Ok(engine)
    }

 /// 修改文件
    pub fn put(&self, key: Bytes, value: Bytes) -> io::Result<()> {
        if key.is_empty() {
            return Err(Error::new(ErrorKind::InvalidInput, "key is empty"));
        }

        let record = LogRecord {
            key: key.to_vec(),
            value: value.to_vec(),
            rec_type: LogRecordType::Normal,
        };

        // 1. 编码并写入磁盘
        let (enc_data, _) = record.encode();
        let enc_len = enc_data.len() as u64;

        // 如果空间不足，先轮转 active file
        self.ensure_active_space(enc_len)?;

        // 获取 active_file 的写锁，仅持有到写入结束
        let (file_id, write_off) = {
            let mut active_file = self.active_file.write();
            let off = active_file.write(&enc_data)?;
            if self.options.sync_writes {
                active_file.sync()?;
            }
            (active_file.get_file_id(), off)
        };
        // 旧值的位置信息
        let old_val: Option<LogRecordPos> = self.index.get(&key);

        // 如果旧值存在，那么旧值被覆盖，需要做垃圾记录和处理
        if let Some(old_pos) = old_val {
            // 更新对应文件的无效数据计数
            let mut reclaim_map = self.file_reclaim_size.write();
            *reclaim_map.entry(old_pos.file_id).or_default() += old_pos.size as u64; 
        }
        // 构造索引位置信息
        let pos = LogRecordPos {
            file_id,
            offset: write_off,
            size: enc_data.len() as u32, 
        };

        // 2. 更新内存索引
        // 注意：先写磁盘，后更新索引。如果写磁盘成功但更新索引失败（比如崩溃），
        // 重启时通过回放文件可以恢复索引，不会丢数据。
        self.index.put(key.to_vec(), pos);

        // 尝试触发合并
        self.maybe_trigger_merge()?;

        Ok(())
    }


    /// 根据Key来获取Value
    pub fn get(&self, key: Bytes) -> io::Result<Option<Bytes>> {
        if key.is_empty() {
             return Ok(None);
        }

        // 1. 从内存索引查找位置
        let pos = match self.index.get(&key) {
            Some(pos) => pos,
            None => return Ok(None),
        };

        // 2. 根据 file_id 确定去哪个文件读
        // 这一步需要处理并发：
        // 如果是 active_file，需要获取 active_file 的读锁。
        // 如果是 older_files，需要获取 map 的读锁，拿到 DataFile 引用。
        
        let active_file = self.active_file.read();
        if active_file.get_file_id() == pos.file_id {
            // 在活跃文件中
            let record = active_file.read_log_record(pos.offset)?;
            
            if record.rec_type == LogRecordType::Deleted {
                debug_assert!(false, "index inconsistent: tombstone encountered for live key");
                return Err(Error::new(ErrorKind::InvalidData, "inconsistent index: key marked deleted"));
            }
            return Ok(Some(Bytes::from(record.value)));
        }

        // 在旧文件中
        let older_files = self.older_files.read();
        let data_file = older_files.get(&pos.file_id).ok_or(
            Error::new(ErrorKind::NotFound, "data file not found")
        )?;
        
        let record = data_file.read_log_record(pos.offset)?;
        
        if record.rec_type == LogRecordType::Deleted {
             debug_assert!(false, "index inconsistent: tombstone encountered for live key");
             return Err(Error::new(ErrorKind::InvalidData, "inconsistent index: key marked deleted"));
        }
        
        Ok(Some(Bytes::from(record.value)))
    }

fn load_index_from_files(&self) -> io::Result<()> {
        let mut file_valid_size = HashMap::new();
        let file_ids = self.file_ids.read();
        // 由于 file_ids 是有序的，我们按顺序遍历
        for (i, &file_id) in file_ids.iter().enumerate() {
            let mut offset = 0;
            loop {
                // 读取 LogRecord
                // 这里有一个技巧：我们需要 DataFile 对象来读取。
                // 启动阶段是单线程的，所以我们不用担心锁竞争，但为了复用代码：
                let log_record_res = if i == file_ids.len() - 1 {
                    // 最后一个是 active file
                    let active_file = self.active_file.read();
                    active_file.read_log_record(offset)
                } else {
                    // 旧文件
                    let older_files = self.older_files.read();
                    let file = older_files.get(&file_id).unwrap();
                    file.read_log_record(offset)
                };

                let (log_record, size) = match log_record_res {
                    Ok(record) => {
                        // 我们需要重新计算 size，因为 read_log_record 只返回了结构体
                        let size = record.encoded_length(); // 假设 LogRecord 实现了这个 helper
                        (record, size)
                    }
                    Err(e) => {
                        if e.kind() == ErrorKind::UnexpectedEof {
                            break; // 文件读完了
                        }
                        return Err(e);
                    }
                };

                // 构建内存索引
                let pos = LogRecordPos {
                    file_id,
                    offset,
                    size: size as u32,
                };

                match log_record.rec_type {
                    LogRecordType::Normal => {
                        self.index.put(log_record.key, pos);
                        *file_valid_size.entry(file_id).or_insert(0) += size as u64;
                    }
                    LogRecordType::Deleted => {
                        // 遇到墓碑值，从索引中删除
                        self.index.delete(&log_record.key);
                    }
                }

                offset += size as u64;
            }
        // 2. 计算无效数据大小
        // 公式：无效大小 = 文件总物理大小 - 有效数据大小
        let mut reclaim_map = self.file_reclaim_size.write();
        let older_files = self.older_files.read();
        
        for (file_id, data_file) in older_files.iter() {
            let valid = file_valid_size.get(file_id).copied().unwrap_or(0);
            let total = data_file.get_write_offset(); 
            if total == 0{
                return Err(Error::new(ErrorKind::Other, "invalid file: metadata is invalid"))
            }
            if total > valid {
                reclaim_map.insert(*file_id, total - valid);
            }
        }
        }
        Ok(())
    }

 /// 删除某一条记录
    pub fn delete(&self, key: Bytes) -> io::Result<()> {
        if key.is_empty() {
             return Err(Error::new(ErrorKind::InvalidInput, "Key is empty"));
        }

        // 1. 检查 Key 是否存在
        if self.index.get(&key).is_none() {
             return Err(Error::new(ErrorKind::NotFound, "Key not found"));
        }
        // 更新删除计数
        let old_val = self.index.get(&key);
        if let Some(old_pos) = old_val {
            let mut reclaim_map = self.file_reclaim_size.write();
            *reclaim_map.entry(old_pos.file_id).or_default() += old_pos.size as u64;
        }
        // 2. 构造 Tombstone 记录
        let record = LogRecord {
            key: key.to_vec(),
            value: vec![], // Value 为空
            rec_type: LogRecordType::Deleted,
        };

        // 3. 写入文件
        let (enc_data, _) = record.encode();
        let enc_len = enc_data.len() as u64;

        // 删除记录也可能导致文件满，先检查
        self.ensure_active_space(enc_len)?;

        {
            let mut active_file = self.active_file.write();
            active_file.write(&enc_data)?;
            
            if self.options.sync_writes {
                active_file.sync()?;
            }
        }

        // 4. 从内存索引删除
        self.index.delete(&key);

        // 尝试触发合并
        self.maybe_trigger_merge()?;

        Ok(())
    }



/// 获取要合并的旧文件集合
    fn get_merge_candidates(&self) -> Vec<u32> {
        let threshold = self.options.compaction_file_threshold;
        let mut candidates = Vec::new();
        
        let reclaim_map = self.file_reclaim_size.read();
        // 我们只看 old files，不看 active file
        let older_files = self.older_files.read();

        for (file_id, &invalid_size) in reclaim_map.iter() {
            if let Some(file) = older_files.get(file_id) {
                let total_size = file.get_write_offset();
                if total_size > 0 {
                    let ratio = invalid_size as f64 / total_size as f64;
                    if ratio >= threshold {
                        candidates.push(*file_id);
                    }
                }
            }
        }
        candidates
    }

    /// 检查是否需要合并，并用 CAS 防止并发触发
    fn maybe_trigger_merge(&self) -> io::Result<()> {
        // 只有存在候选文件才尝试合并
        let candidates_exist = !self.get_merge_candidates().is_empty();
        if !candidates_exist {
            return Ok(());
        }

        // CAS 抢占合并权
        if self
            .merge_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            // 无论成功与否都要清理标记
            let res = self.merge();
            self.merge_running.store(false, Ordering::SeqCst);
            res?;
        }

        Ok(())
    }

    /// 如果 active file 空间不足，执行轮转：同步、移入 older，创建新文件并更新 ID
    fn ensure_active_space(&self, needed: u64) -> io::Result<()> {
        {
            let active = self.active_file.read();
            if active.get_write_offset() + needed <= self.options.data_file_size {
                return Ok(());
            }
        }

        let mut active = self.active_file.write();
        if active.get_write_offset() + needed <= self.options.data_file_size {
            return Ok(());
        }

        active.sync()?;
        let old_file_id = active.get_file_id();

        // 计算新的文件 ID（递增）
        let new_file_id = {
            let file_ids = self.file_ids.read();
            file_ids.last().copied().unwrap_or(old_file_id) + 1
        };

        let new_active = DataFile::new(&self.options.dir_path, new_file_id)?;
        let old_file = std::mem::replace(&mut *active, new_active);

        {
            let mut older = self.older_files.write();
            older.insert(old_file_id, old_file);
        }
        {
            let mut file_ids = self.file_ids.write();
            file_ids.push(new_file_id);
        }

        Ok(())
    }

    /// 将所有合并的文件写入磁盘
    fn write_merge_finished_file(&self, merge_path: &PathBuf, merge_files: &[u32]) -> io::Result<()> {
        let merge_fin_file = merge_path.join(MERGE_FINISHED_FILE_NAME);
        
        // 1. 创建文件
        let mut f = fs::File::create(merge_fin_file)?;

        // 2. 遍历写入 ID
        // 使用 Big Endian (网络字节序) 写入，保证跨平台一致性
        for &file_id in merge_files {
            f.write_all(&file_id.to_be_bytes())?;
        }

        // 3. 【关键】强制刷盘
        // 这一步至关重要。必须确保这些元数据持久化到磁盘，
        // 否则如果机器此时掉电，文件可能是空的，导致合并状态丢失。
        f.sync_all()?;

        Ok(())
    }

    /// 合并旧文件
    pub fn merge(&self) -> io::Result<()> {

        // ==================================================================================
        // 阶段 1: Selection (筛选) - 耗时极短
        // ==================================================================================
        
        // 1.1 获取需要合并的文件 ID 列表
        // 这里调用之前实现的基于碎片率的筛选逻辑
        let merge_candidates = self.get_merge_candidates();
        if merge_candidates.is_empty() {
            return Ok(());
        }
        
        // 使用 HashSet 方便后续快速查找 (O(1))
        let merge_candidates_set: HashSet<u32> = merge_candidates.iter().copied().collect();
        // println!("Compaction started. Candidates: {:?}", merge_candidates);

        // ==================================================================================
        // 阶段 2: Execution (执行) - 耗时最长，无锁，不阻塞主线程
        // ==================================================================================
        
        // 2.1 准备工作目录
        let merge_path = self.options.dir_path.join(MERGE_DIR_NAME);
        if merge_path.exists() {
            fs::remove_dir_all(&merge_path)?;
        }
        fs::create_dir_all(&merge_path)?;

        // 2.2 初始化 MergeContext (负责写文件、自动切分)
        let mut ctx = MergeContext::new(
            merge_path.clone(), 
            self.options.data_file_size // 沿用配置的文件大小限制
        )?;

        // 2.3 遍历待合并的旧文件
        for &file_id in &merge_candidates {
            let older_files = self.older_files.read();
            let data_file = match older_files.get(&file_id) {
                Some(f) => f,
                None => continue, // 极罕见情况：文件在 merge 刚开始时被删了
            };

            let mut offset = 0;
            loop {
                // 读取记录
                let (record, size) = match data_file.read_log_record(offset) {
                    Ok(res) => {
                        let record_size = res.encoded_length();
                        (res,record_size)
                    }
                    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                    Err(e) => return Err(e),
                };

                // --- 核心逻辑：有效性检查 ---
                // 我们拿着读出来的 Key，去内存索引查查看：
                // "现在的索引是不是还指向我自己？"
                // 如果索引指向了别的文件，或者别的 offset，说明这条数据已经是旧的了，直接丢弃。
                let key_bytes = record.key.clone();
                let index_pos = self.index.get(&key_bytes);
                
                let is_valid = if let Some(pos) = index_pos {
                    pos.file_id == file_id && pos.offset == offset
                } else {
                    false
                };

                if is_valid {
                    // 有效数据，写入 MergeContext
                    // Context 会自动处理文件切分、sync 等逻辑
                    ctx.append(record.key, &record.value, LogRecordType::Normal)?;
                }

                offset += size as u64;
            }
        }

        // 2.4 完成 Execution 阶段
        // 获取生成的临时文件 ID 列表 (0, 1, 2...) 和 新的索引位置信息
        // 注意：此时 DataFile 句柄已被 Drop，文件已关闭，可以安全 rename
        let (merge_temp_file_ids, new_positions) = ctx.finish()?;

        // ==================================================================================
        // 阶段 3: Commit (提交) - 获取写锁，耗时短，原子操作
        // ==================================================================================
        
        // 3.1 写入 FINISHED 文件
        // 如果系统在这里崩溃，重启时发现有 FINISHED 文件，但 ID 没对上，说明 Merge 成功了一半。
        // 我们这里存入被合并的 old_file_ids，重启恢复逻辑可以据此清理。
        self.write_merge_finished_file(&merge_path, &merge_candidates)?;

        // 3.2 【Stop The World】获取全局写锁
        // 在我们的架构中，put/delete 都需要 active_file 的写锁。
        // 拿到这个锁，意味着暂停了所有的写入操作。
        let mut _write_lock = self.active_file.write(); 

        // 3.3 确定新的 File ID 起始值
        // 我们要将 merge 目录下的文件搬到主目录，ID 必须是全局唯一的递增值
        let mut file_ids = self.file_ids.write();
        let max_id = *file_ids.last().unwrap_or(&0);
        let start_merge_id = max_id + 1;

        // 3.4 移动文件 & 加载到 Engine
        let mut older_files_map = self.older_files.write();

        for (i, temp_file) in merge_temp_file_ids.iter().enumerate() {
             let temp_id = temp_file.get_file_id();
            let new_id = start_merge_id + i as u32;
            
            // Rename: .merge/0.data -> /data_dir/100.data
            let src_path = DataFile::format_file_name(&merge_path, temp_id);
            let dst_path = DataFile::format_file_name(&self.options.dir_path, new_id);
            fs::rename(&src_path, &dst_path)?;

            // 打开新文件并放入 older_files 映射
            let new_data_file = DataFile::new(&self.options.dir_path, new_id)?;
            older_files_map.insert(new_id, new_data_file);
            
            // 记录 ID
            file_ids.push(new_id);
        }


        let merge_new_count = merge_temp_file_ids.len() as u32;
        let new_active_id = start_merge_id + merge_new_count;

        let new_active_file = DataFile::new(&self.options.dir_path, new_active_id)?;

        let old_active = std::mem::replace(&mut *_write_lock, new_active_file);

        let old_active_id = old_active.get_file_id();
        older_files_map.insert(old_active_id, old_active);
        file_ids.push(old_active_id);
        file_ids.push(new_active_id);


        // 3.5 【Double Check】更新内存索引
        // 这是防止“数据回滚”的最后一道防线
        for (key, mut pos) in new_positions {
            // 修正 File ID：临时 ID -> 全局 ID
            // new_positions 里存的 file_id 是 0, 1...，我们要改成 start_merge_id + 0...
            pos.file_id = start_merge_id + pos.file_id;

            // 查询当前内存索引
            let current_pos = self.index.get(&key);
            
            // 只有当：
            // 1. Key 依然存在
            // 2. 且 Key 指向的文件 ID 依然在“被合并的文件列表”中
            // 我们才更新索引。
            // 
            // 如果 Key 指向了 start_merge_id 之前也没在 candidates 里的文件（即活跃文件），
            // 说明在 Execution 阶段，用户更新了这个 Key。我们需要保留用户的更新，丢弃 Merge 的结果。
            if let Some(curr) = current_pos {
                if merge_candidates_set.contains(&curr.file_id) {
                    self.index.put(key, pos);
                }
            }
        }

        // 3.6 清理旧文件
        // 此时索引已更新，旧文件彻底变成垃圾
        let mut reclaim_map = self.file_reclaim_size.write();
        for &old_id in &merge_candidates {
            // 从内存 map 移除
            older_files_map.remove(&old_id);
            reclaim_map.remove(&old_id);
            
            // 从 file_ids 移除
            if let Some(idx) = file_ids.iter().position(|&x| x == old_id) {
                file_ids.remove(idx);
            }
            
            // 从磁盘删除
            let file_path = DataFile::format_file_name(&self.options.dir_path, old_id);
            // 忽略删除错误（可能是文件句柄未释放等偶发问题，不影响数据正确性）
            let _ = fs::remove_file(file_path);
        }

        // 3.7 清理 Merge 目录
        fs::remove_dir_all(merge_path)?;
        // println!("Merge finished. Reclaimed {} files.", merge_candidates.len());
        Ok(())
    }
}

// 辅助函数：加载目录下的所有 .data 文件 ID
fn load_data_file_ids(dir_path: &Path) -> io::Result<Vec<u32>> {
    let mut file_ids = Vec::new();
    let read_dir = fs::read_dir(dir_path)?;
    
    for entry in read_dir {
        let entry = entry?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        
        // 假设文件名格式固定为 000001.data
        if name.ends_with(".data") {
            // 解析 ID
            let id: u32 = name.trim_end_matches(".data").parse().map_err(|_| {
                 Error::new(ErrorKind::InvalidData, "invalid data file name")
            })?;
            file_ids.push(id);
        }
    }
    // 排序，保证从小到大加载
    file_ids.sort();
    Ok(file_ids)
}


impl KvsEngine for KVEngine{
    fn set(&mut self, key: Bytes, value: Bytes) -> io::Result<()> {
        self.put(key, value)
    }
    fn get(&mut self, key: Bytes) -> io::Result<Option<Bytes>> {
        KVEngine::get(self, key)
    }
    fn remove(&mut self, key: Bytes) -> io::Result<()> {
        self.delete(key)
    }
}



















#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::io::ErrorKind;
    use tempfile::tempdir;

    fn make_opts(dir: &Path) -> Options {
        let mut opts = Options::default();
        opts.dir_path = dir.to_path_buf();
        opts.data_file_size = 64; // 小文件便于测试
        opts.sync_writes = false;
        opts
    }

    #[test]
    fn put_and_get_round_trip() -> io::Result<()> {
        let dir = tempdir()?;
        let engine = KVEngine::open(make_opts(dir.path()))?;

        engine.put(Bytes::from("k1"), Bytes::from("v1"))?;
        let v = engine.get(Bytes::from("k1"))?;
        assert_eq!(v, Some(Bytes::from("v1")));
        Ok(())
    }

    #[test]
    fn delete_makes_key_not_found() -> io::Result<()> {
        let dir = tempdir()?;
        let engine = KVEngine::open(make_opts(dir.path()))?;

        engine.put(Bytes::from("k"), Bytes::from("v"))?;
        engine.delete(Bytes::from("k"))?;
        let err = engine.get(Bytes::from("k")).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        Ok(())
    }

    #[test]
    fn open_respects_create_if_missing() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("no_such_dir");

        // create_if_missing = false 应报错
        // let mut opts = make_opts(&missing);
        // opts.create_if_missing = false;
        // let err = Engine::open(opts);
        // if let Err() 
        // assert_eq!(err.kind(), ErrorKind::NotFound);

        // create_if_missing = true 应自动创建成功
        let mut opts_ok = make_opts(&missing);
        opts_ok.create_if_missing = true;
        KVEngine::open(opts_ok).expect("should create missing dir");
    }

    #[test]
    fn merge_compacts_old_file_and_updates_index() -> io::Result<()> {
        let dir = tempdir()?;
        let mut opts = make_opts(dir.path());
        opts.compaction_file_threshold = 0.1; // 低阈值方便触发
        let engine = KVEngine::open(opts)?;

        // 准备一个旧文件 id=1，写入一条记录
        let mut old_file = DataFile::new(dir.path(), 1)?;
        let rec = LogRecord {
            key: b"mk".to_vec(),
            value: b"mv".to_vec(),
            rec_type: LogRecordType::Normal,
        };
        let (enc, size) = rec.encode();
        let offset = old_file.write(&enc)?;
        old_file.sync()?;

        // 注入到 older_files 与 file_ids
        engine.older_files.write().insert(1, old_file);
        engine.file_ids.write().push(1);

        // 索引指向旧文件
        let pos = LogRecordPos {
            file_id: 1,
            offset,
            size: size as u32,
        };
        engine.index.put(b"mk".to_vec(), pos);

        // 设置回收比例，让文件进入合并候选
        engine.file_reclaim_size.write().insert(1, size as u64);

        // 执行合并
        engine.merge()?;

        // 合并后，索引应指向新文件（id 从 2 开始），偏移重置为 0
        let new_pos = engine.index.get(b"mk").expect("index missing after merge");
        assert_eq!(new_pos.file_id, 2);
        assert_eq!(new_pos.offset, 0);

        // 旧文件应被清理
        assert!(engine.older_files.read().get(&1).is_none());
        // 新文件已加载
        assert!(engine.older_files.read().get(&2).is_some());

        Ok(())
    }
}
