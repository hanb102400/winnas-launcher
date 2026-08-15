//! 文件日志：程序启动时生成 `conf/logs/YYYY-MM-DD.log`，记录程序触发操作的日志。
//!
//! - 文件名按当前日期命名（如 `2026-08-15.log`）
//! - 每条日志带时间戳 + 标签 + 内容，追加写入
//! - 同时输出到 stderr（dev 模式 console 可见，release 无 console 也不报错）

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Local;

static LOG_FILE: Mutex<Option<File>> = Mutex::new(None);

fn log_path() -> PathBuf {
    let date = Local::now().format("%Y-%m-%d").to_string();
    super::system_state::conf_dir()
        .join("logs")
        .join(format!("{date}.log"))
}

/// 初始化日志文件（启动时调用一次，追加模式）。
pub fn init() {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();
    *LOG_FILE.lock().unwrap() = file;
    write("boot", &format!("日志初始化 {}", path.display()));
}

/// 写一条日志（时间戳 + 标签 + 内容）。
pub fn info(tag: &str, msg: &str) {
    write(tag, msg);
}

fn write(tag: &str, msg: &str) {
    let ts = Local::now().format("%H:%M:%S").to_string();
    let line = format!("[{ts}] [{tag}] {msg}");
    if let Some(f) = LOG_FILE.lock().unwrap().as_mut() {
        let _ = f.write_all(line.as_bytes());
        let _ = f.write_all(b"\n");
        let _ = f.flush();
    }
    // 同时输出到 stderr，便于 dev 模式实时观察
    eprintln!("[{tag}] {msg}");
}
