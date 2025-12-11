use crate::record::log_record::{
    decode_log_record_header, get_log_record_crc, LogRecord, LogRecordType,
    MAX_LOG_RECORD_HEADER_SIZE,
};
use std::fs::{File, OpenOptions};
use std::io::{self,Write};
use std::path::{Path, PathBuf};
use crate::record::dbio::{FileReadAt};

/// 文件后缀名定义
pub const DATA_FILE_NAME_SUFFIX: &str = ".data";

/// DataFile 封装底层文件操作
pub struct DataFile {
    file_id: u32,       // 文件 ID
    offset: u64,        // 当前写入偏移量，用于追加写
    file: File,         // 标准文件句柄
}

impl DataFile{
    /// 创建或打开一个新的数据文件
    pub fn new(dir_path: &Path, file_id: u32) -> io::Result<Self> {
        // 构造文件名: 000001.data
        let file_name = format!("{:09}{}", file_id, DATA_FILE_NAME_SUFFIX);
        let path = dir_path.join(file_name);

        // 打开文件：如果不存在则创建，支持读写和追加
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .append(true)
            .open(&path)?;

        // 获取当前文件大小，作为初始 offset
        let offset = file.metadata()?.len();

        Ok(DataFile {
            file_id,
            offset,
            file,
        })
    }
    
    /// 返回文件大小，如果错误返回0
    // pub fn file_size(&self) -> u64{
    //     if let Ok(Len) = self.file.metadata(){
    //         Len.len()
    //     }
    //     else{
    //         0 as u64
    //     }
    // }


    /// 获取文件的ID，getter访问器
    pub fn get_file_id(&self) -> u32 {
        self.file_id
    }

    /// 格式化文件名
    pub fn format_file_name(dir: &Path, id: u32) -> PathBuf {
        let name = format!("{:09}{}", id, DATA_FILE_NAME_SUFFIX);
        dir.join(name)
    }
    
    /// 获取文件目前的写偏移量，继续确认文件大小
    pub fn get_write_offset(&self) -> u64 {
        self.offset
    }

    /// 写入 LogRecord 到磁盘
    /// 返回值: (起始写入位置, 写入的总字节数)
    pub fn write(&mut self, buf: &[u8]) -> io::Result<u64> {
        // 标准 I/O 的 write_all 会自动处理循环写入
        self.file.write_all(buf)?;

        let write_pos = self.offset;
        self.offset += buf.len() as u64;

        Ok(write_pos)
    }

    /// 持久化数据到磁盘 (fsync)
    pub fn sync(&self) -> io::Result<()> {
        self.file.sync_all()
    }

    /// 根据偏移量读取一条Record
    pub fn read_log_record(&self, offset: u64) -> io::Result<LogRecord> {
        // 1. 获取文件的临时读取句柄。考虑到并发场景，封装了Windows下和Unix下的无游标读
        let read_file = &self.file; 
        
        // 2. 读取 Header

        // 读取头部字节
        let mut header_buf = [0u8; MAX_LOG_RECORD_HEADER_SIZE];
        // 如果读不到 header，说明可能遇到 EOF 或者文件损坏
        read_file.read_at_exact(&mut header_buf, offset)?;

        // 3. 解析 Header
        let (key_size, value_size, rec_type, saved_crc) = decode_log_record_header(&header_buf);

        // 4. 读取 Key 和 Value
        let key_size = key_size as usize;
        let value_size = value_size as usize;
        let total_size = key_size + value_size;

        let mut body_buf = vec![0u8; total_size];
        read_file.read_at_exact(&mut body_buf, offset + MAX_LOG_RECORD_HEADER_SIZE as u64)?;

        // 5. 构造 LogRecord
        let log_record = LogRecord {
            key: body_buf[0..key_size].to_vec(),
            value: body_buf[key_size..].to_vec(),
            rec_type,
        };

        // 6. 校验 CRC
        // CRC 校验范围：Header(除去前4字节CRC) + Key + Value
        // header_buf[4..] 是 type + key_size + val_size
        let calculated_crc = get_log_record_crc(&log_record, &header_buf[4..]);

        if calculated_crc != saved_crc {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "LogRecord CRC check failed: data corrupted",
            ));
        }

        Ok(log_record)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir; // 需要添加 dev-dependency: tempfile

    #[test]
    fn test_data_file_write_and_read() {
        let dir = tempdir().unwrap();
        let mut data_file = DataFile::new(dir.path(), 1).expect("failed to create data file");

        // 1. 构造记录
        let rec = LogRecord {
            key: "key-1".into(),
            value: "value-1".into(),
            rec_type: LogRecordType::Normal,
        };
        let (enc_data, _size) = rec.encode();
        // 2. 写入
        let offset = data_file.write(&enc_data).expect("write failed");
        assert_eq!(offset, 0);

        // 3. 读取
        let read_rec = data_file.read_log_record(offset).expect("read failed");
        assert_eq!(read_rec.key, rec.key);
        assert_eq!(read_rec.value, rec.value);
        assert_eq!(read_rec.rec_type, rec.rec_type);
    }
}