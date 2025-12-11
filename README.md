# MemKV (Rust)

一个基于 **Bitcask** 思想改造的内存 / 磁盘混合 KV 存储引擎。当前实现为单线程同步版本，主要用于教学与实验；后续将引入多线程/并发处理，提升吞吐。

## 设计要点
- **WAL + 索引**：采用追加式日志文件存储数据，内存中维护 Hash 索引（Key → 位置），重启时通过日志重放恢复。
- **Active/Older 文件模型**：当前活跃文件负责写入，历史文件只读，便于顺序写和读放大控制。
- **压缩/合并**：跟踪无效数据比例，超过阈值触发文件合并，回收空间。
- **引擎可插拔**：通过 `KvsEngine` trait，支持自研 `KVEngine` 与 `SledKvsEngine` 两种后端。
- **JSON 协议 + TCP**：服务器使用 JSON 进行请求/响应编解码，通信简单、易调试。

## 快速开始
```bash
# 构建
cargo build

# 启动服务器（默认引擎 kvs，监听 127.0.0.1:4000）
cargo run --bin kvs-server

# 选择 sled 引擎
cargo run --bin kvs-server -- --engine sled --addr 127.0.0.1:4005

# 使用客户端
cargo run --bin kvs-client -- set key value
cargo run --bin kvs-client -- get key
cargo run --bin kvs-client -- rm key
```

## 目录结构
- `src/engines/engine.rs`：自研 Bitcask 风格引擎 `KVEngine`
- `src/engines/sled.rs`：基于 sled 的适配器
- `src/server.rs`：TCP 服务器，基于 `KvsEngine` 抽象
- `src/client.rs`：TCP 客户端
- `tests/cli.rs`：端到端 CLI 测试

## 当前限制
- **单线程**：监听循环与请求处理均为串行，吞吐有限。
- **简单协议**：无鉴权/压缩/批量操作，主要用于学习与验证。

## 未来规划
1. 多线程/线程池：并发处理连接，请求分发到共享引擎。
2. 更丰富索引选择：如 B+Tree、跳表。
3. 后台合并与速率控制：降低写入抖动。
4. 观测性：Prometheus 指标、结构化日志。

## 测试
```bash
cargo test
```

## 许可证
MIT
