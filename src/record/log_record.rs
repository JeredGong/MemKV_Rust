use bytes::{BufMut, BytesMut};
use crc32fast::Hasher;
use std::convert::TryInto;
/// Header 的长度: CRC(4) + Type(1) + KeySize(4) + ValueSize(4) = 13
pub const MAX_LOG_RECORD_HEADER_SIZE: usize = 13;

/// 标记位,用于指示本条Log是否有效
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum LogRecordType {
    /// 正常状态，记录有效
    Normal = 1,

    /// 删除状态，记录无效 
    Deleted = 2,
}

impl LogRecordType {
    /// 将 u8 转换为 Enum
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => LogRecordType::Normal,
            2 => LogRecordType::Deleted,
            _ => panic!("unsupported log record type"),  // TODO:这里建议使用Result处理标记位错误的问题
        }
    }
}


/// 记录的位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogRecordPos {
    /// 文件 ID
    pub file_id: u32,
    /// 偏移量 
    pub offset: u64, 
    /// Value大小 
    pub size: u32,    
}


/// 写入到磁盘文件的每一条记录的结构
/// 物理存储格式为：
/// +-------+--------+----------+------------+-----------+-------+---------+
/// |  CRC  |  Type  | KeySize  | ValueSize  | Key       | Value |
/// +-------+--------+----------+------------+-----------+-------+---------+
/// |  4B   |   1B   |   4B     |   4B       | ...       | ...   |
/// +-------+--------+----------+------------+-----------+-------+---------+
#[derive(Debug)]
pub struct LogRecord {
    /// Key的字节表示
    pub key: Vec<u8>,
    /// Value的字节表示
    pub value: Vec<u8>,
    /// 标识记录是否有效
    pub rec_type: LogRecordType,
}
impl LogRecord {
    /// 编码 LogRecord 为字节向量
    /// 返回值: (编码后的字节数据, 数据总长度)
    pub fn encode(&self) -> (Vec<u8>, usize) {
        // 1. 初始化 Encode Buffer
        // Header = CRC(4) + Type(1) + KeySize(4) + ValueSize(4)
        let serialized_size = self.key.len() + self.value.len() + 13;
        let mut buf = BytesMut::with_capacity(serialized_size);

        // 2. 预留 CRC 的位置 (4字节)，后续填入
        buf.put_u32(0); 

        // 3. 写入 Type
        buf.put_u8(self.rec_type as u8);

        // 4. 写入 KeySize 和 ValueSize (使用变长 varint 可以省空间，这里为简单使用 u32)
        // TODO：需要处理 usize 转 u32 的截断风险，虽然 4GB key 很少见
        buf.put_u32(self.key.len() as u32);
        buf.put_u32(self.value.len() as u32);

        // 5. 写入 Key 和 Value
        buf.extend_from_slice(&self.key);
        buf.extend_from_slice(&self.value);

        // 6. 计算 CRC
        // 注意：CRC 计算的数据范围是 [Type, KeySize, ValueSize, Key, Value]
        let mut hasher = Hasher::new();
        hasher.update(&buf[4..]); // 跳过前4个字节(CRC占位符)
        let crc = hasher.finalize();

        // 7. 回填 CRC 到 buffer 开头
        let mut vec = buf.to_vec();
        vec[0..4].copy_from_slice(&crc.to_be_bytes()); // 大端序写入

        (vec, serialized_size)
    }

    /// 辅助函数，计算长度
    pub fn encoded_length(&self) -> usize{
        4 + 1 + 4 + 4 + self.key.len() + self.value.len()
    }
}


/// 从 Header 字节中解析元数据
pub fn decode_log_record_header(buf: &[u8]) -> (u32, u32, LogRecordType, u32) {
    
    // 1. 读取 CRC (前4字节)
    let crc = u32::from_be_bytes(buf[0..4].try_into().unwrap());
    
    // 2. 读取 Type
    let rec_type = LogRecordType::from_u8(buf[4]);

    // 3. 读取 KeySize 和 ValueSize
    let key_size = u32::from_be_bytes(buf[5..9].try_into().unwrap());
    let value_size = u32::from_be_bytes(buf[9..13].try_into().unwrap());

    (key_size, value_size, rec_type, crc)
}

/// 校验 CRC
pub fn get_log_record_crc(rec: &LogRecord, header: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    // 这里的 header 是不包含 CRC 的部分 (即 buf[4..])，实际调用时需要小心传入
    hasher.update(header); 
    hasher.update(&rec.key);
    hasher.update(&rec.value);
    hasher.finalize()
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_record_encode_and_decode() {
        // 1. 创建记录
        let rec = LogRecord {
            key: "name".as_bytes().to_vec(),
            value: "bitcask-rs".as_bytes().to_vec(),
            rec_type: LogRecordType::Normal,
        };

        // 2. 编码
        let (bytes, len) = rec.encode();
        assert_eq!(len, 13 + 4 + 10); // Header(13) + Key(4) + Value(10)
        
        // 3. 模拟解码 Header
        let (k_size, v_size, r_type, crc) = decode_log_record_header(&bytes[0..13]);
        assert_eq!(k_size, 4);
        assert_eq!(v_size, 10);
        assert_eq!(r_type, LogRecordType::Normal);
        
        // 4. 验证 CRC
        // 重新计算 CRC：输入是 Header(不含CRC部分) + Key + Value
        let mut hasher = Hasher::new();
        hasher.update(&bytes[4..13]);
        hasher.update(&rec.key);
        hasher.update(&rec.value);
        let calc_crc = hasher.finalize();
        
        assert_eq!(crc, calc_crc);
    }
}