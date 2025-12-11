use std::fs::File;
use std::io::{self};

/// 一个跨平台的“从指定 offset 把 buf 填满”的 trait
pub trait FileReadAt {

    /// 一个跨平台的无游标读
    fn read_at_exact(&self, buf: &mut [u8], offset: u64) -> io::Result<()>;
}

#[cfg(unix)]
impl FileReadAt for File {
    fn read_at_exact(&self, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
        use std::os::unix::fs::FileExt;

        while !buf.is_empty() {
            let n = FileExt::read_at(self, buf, offset)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected EOF when read_at_exact",
                ));
            }
            offset += n as u64;
            let tmp = buf;
            buf = &mut tmp[n..];
        }
        Ok(())
    }
}

#[cfg(windows)]
impl FileReadAt for File {
    fn read_at_exact(&self, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
        use std::os::windows::fs::FileExt;

        while !buf.is_empty() {
            let n = FileExt::seek_read(self, buf, offset)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected EOF when read_at_exact",
                ));
            }
            offset += n as u64;
            let tmp = buf;
            buf = &mut tmp[n..];
        }
        Ok(())
    }
}
