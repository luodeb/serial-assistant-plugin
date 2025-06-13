# 串口调试助手插件

## 简介

串口调试助手插件是一个基于 Rust 和 Tokio 的高性能串口通信插件，专为 Tauri 聊天客户端项目设计。它提供了完整的串口通信功能，包括端口管理、数据收发、配置管理和实时监控。

## 特性

- 🔌 **多端口支持**: 自动发现和管理系统中的所有串口设备
- ⚡ **异步高性能**: 基于 Tokio 异步运行时，支持高并发串口操作
- 📊 **实时监控**: 提供连接状态、数据统计和性能监控
- 🔧 **灵活配置**: 支持波特率、数据位、停止位、校验位等完整配置
- 📝 **多种格式**: 支持文本、十六进制、二进制数据格式
- 🛡️ **错误处理**: 完善的错误处理和异常恢复机制
- 🔄 **热插拔**: 支持设备的热插拔检测和自动重连

## 架构设计

```
插件架构：
┌─────────────────────────────────────────────────────────────┐
│                    前端 UI 层                                │
│              (SerialDebugger.vue)                          │
└─────────────────────┬───────────────────────────────────────┘
                      │ Plugin Messages
┌─────────────────────▼───────────────────────────────────────┐
│                 插件接口层                                   │
│            (plugin-interfaces)                             │
└─────────────────────┬───────────────────────────────────────┘
                      │ API Calls
┌─────────────────────▼───────────────────────────────────────┐
│               串口调试助手插件                                │
│          (SerialDebugAssistant)                            │
├─────────────────────────────────────────────────────────────┤
│  ├── config.rs      (配置管理)                              │
│  ├── serial_client.rs (串口客户端)                          │
│  ├── error.rs       (错误处理)                              │
│  └── lib.rs         (插件入口)                              │
└─────────────────────┬───────────────────────────────────────┘
                      │ System Calls
┌─────────────────────▼───────────────────────────────────────┐
│                 底层串口库                                   │
│          (tokio-serial + serialport)                       │
└─────────────────────────────────────────────────────────────┘
```

## 技术栈

- **语言**: Rust 2021 Edition
- **异步运行时**: Tokio 1.45.1
- **串口库**: 
  - `serialport 4.7.2` - 跨平台低级串口库
  - `tokio-serial 5.4.5` - Tokio 异步串口实现
- **序列化**: serde + serde_json
- **错误处理**: thiserror + anyhow
- **配置管理**: toml
- **日志**: log

## 安装和构建

### 前置要求

- Rust 1.59.0+
- 系统串口驱动支持

### 构建步骤

```bash
# 进入插件目录
cd plugins/serial-debug-assistant

# 构建插件
cargo build --release

# 运行测试
cargo test

# 检查代码
cargo clippy
```

### 依赖说明

插件依赖的主要 crate：

```toml
[dependencies]
# 串口通信库
serialport = "4.7.2"           # 跨平台串口库
tokio-serial = "5.4.5"         # Tokio 异步串口

# 核心依赖
tokio = { version = "1.45.1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# 插件接口
plugin-interfaces = { path = "../../plugin-interfaces" }

# 错误处理
anyhow = "1.0"
thiserror = "1.0"
```

## API 接口

### 可用端口管理

```rust
// 获取可用端口列表
RequestType::GetPorts
ResponseType::Ports { ports: Vec<PortInfo> }

// 刷新端口列表
RequestType::RefreshPorts
ResponseType::Ports { ports: Vec<PortInfo> }
```

### 连接管理

```rust
// 连接串口
RequestType::Connect { config: SerialConfig }
ResponseType::Connected { success: bool, message: String }

// 断开连接
RequestType::Disconnect
ResponseType::Disconnected { success: bool, message: String }

// 获取连接状态
RequestType::GetStatus
ResponseType::Status { status: ConnectionStatus }
```

### 数据通信

```rust
// 发送数据
RequestType::SendData { data: Vec<u8>, format: DataFormat }
ResponseType::DataSent { success: bool, bytes: usize, message: String }

// 接收数据 (异步推送)
ResponseType::DataReceived { packet: DataPacket }
```

### 配置和统计

```rust
// 获取配置
RequestType::GetConfig
ResponseType::Config { config: SerialConfig }

// 获取统计信息
RequestType::GetStatistics
ResponseType::Statistics { stats: Statistics }
```

## 配置选项

### 串口配置 (SerialConfig)

```rust
pub struct SerialConfig {
    pub port: String,                    // 端口名称
    pub baud_rate: u32,                 // 波特率
    pub data_bits: u8,                  // 数据位 (5-8)
    pub stop_bits: u8,                  // 停止位 (1-2)
    pub parity: ParityMode,             // 校验位
    pub flow_control: FlowControl,      // 流控制
    pub read_timeout: Option<Duration>, // 读取超时
    pub write_timeout: Option<Duration>,// 写入超时
}
```

### 支持的配置值

- **波特率**: 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600
- **数据位**: 5, 6, 7, 8
- **停止位**: 1, 2
- **校验位**: None (无), Odd (奇), Even (偶), Mark (标记), Space (空格)
- **流控制**: None (无), Software (软件), Hardware (硬件)

## 数据格式

### 支持的数据格式

```rust
pub enum DataFormat {
    Text,    // 文本格式 - UTF-8 字符串
    Hex,     // 十六进制格式 - "48 65 6C 6C 6F"
    Binary,  // 二进制格式 - "01001000 01100101"
}
```

### 数据包结构

```rust
pub struct DataPacket {
    pub timestamp: String,           // 时间戳
    pub direction: DataDirection,    // 传输方向 (Sent/Received)
    pub data: Vec<u8>,              // 原始数据
    pub format: DataFormat,         // 数据格式
    pub display_text: String,       // 显示文本
}
```

## 统计信息

```rust
pub struct Statistics {
    pub bytes_sent: u64,            // 发送字节数
    pub bytes_received: u64,        // 接收字节数
    pub packets_sent: u64,          // 发送包数
    pub packets_received: u64,      // 接收包数
    pub errors_count: u64,          // 错误次数
    pub connection_time: Option<Duration>, // 连接时长
    pub last_activity: Option<Instant>,   // 最后活动时间
}
```

## 错误处理

### 错误类型

```rust
pub enum SerialError {
    ConnectionFailed(String),       // 连接失败
    InvalidConfiguration(String),   // 配置无效
    SendFailed(String),            // 发送失败
    ReceiveFailed(String),         // 接收失败
    PortEnumerationFailed(String), // 端口枚举失败
    DataFormatError(String),       // 数据格式错误
    Timeout(String),               // 超时错误
    IoError(std::io::Error),       // IO错误
    SerialPortError(serialport::Error), // 串口库错误
    TaskCancelled,                 // 任务取消
    PluginError(String),           // 插件错误
}
```

### 错误响应

所有错误都会被转换为统一的错误响应格式：

```rust
pub struct ErrorResponse {
    pub error_type: String,    // 错误类型
    pub message: String,       // 错误消息
    pub details: Option<String>, // 详细信息
    pub timestamp: String,     // 时间戳
}
```

## 使用示例

### 基本使用流程

1. **获取可用端口**
```json
{
  "type": "GetPorts"
}
```

2. **配置并连接**
```json
{
  "type": "Connect",
  "data": {
    "port": "COM1",
    "baud_rate": 115200,
    "data_bits": 8,
    "stop_bits": 1,
    "parity": "None",
    "flow_control": "None"
  }
}
```

3. **发送数据**
```json
{
  "type": "SendData",
  "data": {
    "data": [72, 101, 108, 108, 111],
    "format": "Text"
  }
}
```

4. **断开连接**
```json
{
  "type": "Disconnect"
}
```

## 性能特性

- **异步 I/O**: 基于 Tokio 的非阻塞 I/O 操作
- **零拷贝**: 最小化数据复制操作
- **缓冲管理**: 智能的读写缓冲区管理
- **资源回收**: 自动的资源清理和内存管理

## 平台支持

- ✅ Windows (COM 端口)
- ✅ Linux (ttyUSB, ttyACM 等)
- ✅ macOS (cu.* 设备)
- ✅ 支持 USB 转串口设备

## 开发和调试

### 启用日志

```rust
// 在主程序中初始化日志
env_logger::init();

// 或者使用自定义配置
env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();
```

### 测试

```bash
# 运行单元测试
cargo test

# 运行集成测试
cargo test --test integration

# 运行基准测试
cargo bench
```

### 调试技巧

1. **启用调试日志**: 设置 `RUST_LOG=debug`
2. **使用虚拟串口**: 在开发环境中使用 socat 或类似工具
3. **模拟设备**: 使用串口模拟器进行测试

## 故障排除

### 常见问题

1. **端口被占用**
   - 检查是否有其他程序使用该端口
   - 确保端口权限正确

2. **连接超时**
   - 检查设备是否正确连接
   - 验证配置参数是否正确

3. **数据乱码**
   - 检查波特率设置
   - 验证数据位和校验位配置

4. **权限问题 (Linux/macOS)**
   ```bash
   sudo usermod -a -G dialout $USER
   sudo chmod 666 /dev/ttyUSB0
   ```

## 许可证

本项目采用 MIT 许可证，详见 LICENSE 文件。

## 贡献

欢迎提交 Pull Request 和 Issue！请确保：

1. 代码符合 Rust 风格指南
2. 添加适当的测试
3. 更新相关文档
4. 通过所有 CI 检查

## 更新日志

### v1.0.0 (2025-06-13)
- 初始版本发布
- 基本串口通信功能
- 完整的 API 接口
- 错误处理和日志记录
- 配置管理和统计信息
