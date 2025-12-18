# MemKV (Rust)

一个基于 **Bitcask** 思想改造的内存 / 磁盘混合 KV 存储引擎，支持多线程并发处理客户端连接，主要用于教学与实验。

## 设计要点
- **WAL + 索引**：采用追加式日志文件存储数据，内存中维护 Hash 索引（Key → 位置），重启时通过日志重放恢复。
- **Active/Older 文件模型**：当前活跃文件负责写入，历史文件只读，便于顺序写和读放大控制。
- **压缩/合并**：跟踪无效数据比例，超过阈值触发文件合并，回收空间。
- **引擎可插拔**：通过 `KvsEngine` trait，支持自研 `KVEngine` 与 `SledKvsEngine` 两种后端。
- **JSON 协议 + TCP**：服务器使用 JSON 进行请求/响应编解码，通信简单、易调试。
- **多线程并发**：服务器将每个连接分发到线程池执行；引擎句柄通过 `Clone` 共享（内部使用同步原语保证并发安全）。

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

## 并发模型说明
- **连接级并发**：每个 TCP 连接由线程池中的一个任务处理，因此可同时服务多个客户端。
- **连接内串行**：同一连接上的请求按 JSON 流顺序逐条处理（保持简单和可预测性）。
- **线程池**：提供 `ThreadPool` trait，以及 `NaiveThreadPool` / `RayonThreadPool` 两种实现；服务器当前默认使用 `RayonThreadPool`，线程数取自 `std::thread::available_parallelism()`（失败则回退为 4）。

## 目录结构
- `src/engines/engine.rs`：自研 Bitcask 风格引擎 `KVEngine`
- `src/engines/sled.rs`：基于 sled 的适配器
- `src/server.rs`：TCP 服务器，基于 `KvsEngine` 抽象
- `src/client.rs`：TCP 客户端
- `src/thread_pool/`：线程池抽象与实现
- `tests/cli.rs`：端到端 CLI 测试
- `tests/thread_pool.rs`：线程池并发行为测试

## 当前限制
- **简单协议**：无鉴权/压缩/批量操作，主要用于学习与验证。
- **线程池不可配置**：当前未提供 CLI 参数来选择线程池实现或设置线程数。

## 未来规划
1. 线程池可配置：通过 CLI 或配置文件选择实现、设置线程数。
2. 更丰富索引选择：如 B+Tree、跳表。
3. 后台合并与速率控制：降低写入抖动。
4. 观测性：Prometheus 指标、结构化日志。

## 测试
```bash
cargo test
```

## 许可证
MIT
