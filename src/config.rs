use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 用户配置结构（来自 serial.toml）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserConfig {
    pub serial: SerialConfig,
    pub ui: UiConfig,
    pub data: DataConfig,
    pub advanced: AdvancedConfig,
}

/// 串口配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SerialConfig {
    pub port: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: Parity,
    pub flow_control: FlowControl,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: String::new(),
            baud_rate: 115200,
            data_bits: 8,
            stop_bits: 1,
            parity: Parity::None,
            flow_control: FlowControl::None,
        }
    }
}

/// 校验位
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum Parity {
    None,
    Even,
    Odd,
}

/// 流控制
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum FlowControl {
    None,
    Hardware,
    Software,
}

/// UI配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiConfig {
    pub auto_scroll: bool,
    pub timestamp: bool,
    pub max_lines: u32,
}

/// 数据配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DataConfig {
    pub format: DataFormat,
    pub encoding: String,
    pub auto_send_interval: u64,
}

/// 数据格式
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum DataFormat {
    Text,
    Hex,
    Binary,
}

/// 高级配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdvancedConfig {
    pub read_timeout: u64,
    pub write_timeout: u64,
    pub buffer_size: usize,
}

/// 加载用户配置文件
pub fn load_user_config() -> Result<UserConfig, Box<dyn std::error::Error>> {
    let config_path = get_user_config_path()?;

    // 如果配置文件不存在，创建默认配置
    if !config_path.exists() {
        let default_config = create_default_user_config();
        save_user_config(&default_config)?;
        return Ok(default_config);
    }

    let content = fs::read_to_string(&config_path)?;
    let config: UserConfig = toml::from_str(&content)?;
    Ok(config)
}

/// 保存用户配置文件
pub fn save_user_config(config: &UserConfig) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = get_user_config_path()?;

    // 确保目录存在
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = toml::to_string_pretty(config)?;
    fs::write(&config_path, content)?;
    Ok(())
}

/// 获取用户配置文件路径（serial.toml）
fn get_user_config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // 在插件目录下查找 serial.toml
    let mut path = std::env::current_exe()?;
    path.pop(); // 移除可执行文件名
    path.push("plugins");
    path.push("serial-debug-assistant");
    path.push("serial.toml");
    Ok(path)
}

/// 创建默认用户配置
pub fn create_default_user_config() -> UserConfig {
    UserConfig {
        serial: SerialConfig {
            port: String::new(),
            baud_rate: 115200,
            data_bits: 8,
            stop_bits: 1,
            parity: Parity::None,
            flow_control: FlowControl::None,
        },
        ui: UiConfig {
            auto_scroll: true,
            timestamp: true,
            max_lines: 1000,
        },
        data: DataConfig {
            format: DataFormat::Hex,
            encoding: "UTF-8".to_string(),
            auto_send_interval: 0,
        },
        advanced: AdvancedConfig {
            read_timeout: 1000,
            write_timeout: 1000,
            buffer_size: 4096,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_user_config() {
        let config = create_default_user_config();
        assert_eq!(config.serial.baud_rate, 115200);
        assert_eq!(config.serial.data_bits, 8);
        assert_eq!(config.ui.max_lines, 1000);
    }

    #[test]
    fn test_config_serialization() {
        let config = create_default_user_config();
        let serialized = toml::to_string(&config).unwrap();
        let deserialized: UserConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(config.serial.baud_rate, deserialized.serial.baud_rate);
    }
}
