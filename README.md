# WinNas Launcher

> 为 Windows x86 迷你主机 + 投影/电视场景设计的 **TV 启动器**，把 Windows 变成类似 Android TV 的媒体中心桌面。

开机直达全屏应用网格，用遥控器/键盘方向键操作，启动你的 Kodi、Jellyfin、播放器、游戏等，无需键鼠。

## ✨ 特性

- 🖥️ **无边框独占全屏** — 开机即全屏置顶，直达 Launcher，不看到 Windows 桌面
- 🎮 **纯遥控器/键盘操作** — 方向键焦点导航、网格分页、应用按使用频率自动排序
- 🚀 **一键启动任意程序** — 支持 `.exe` / `.lnk`，启动后程序自动置顶、退出后自动回 Launcher
- 📺 **网格菜单 + 分页** — 6×3 网格，翻页浏览，自动提取应用图标
- 🔒 **拦截系统快捷键** — 防止误触 Win / Win+D / Alt+F4 等切出 Launcher
- 🔊 **音量控制 + OSD** — 遥控音量键调节系统音量，屏幕 OSD 提示
- ⏻ **系统控制** — 关机 / 重启 / 睡眠 / 锁屏 / 退出
- 🛡️ **崩溃自恢复** — 崩溃自动重启 + 系统状态快照自愈（强杀后下次启动自动恢复任务栏/桌面）
- 💾 **双形态分发** — 常规安装版 + 绿色便携版（不写注册表、不写 APPDATA）

## 🖥️ 界面

- **首页**：居中 6×3 应用网格（分页），右上角时钟 + 设置齿轮
- **设置抽屉**：恢复桌面、清除菜单缓存、开机自启、添加 APP、按键说明、系统操作、退出、维护模式
- **首次启动引导**：选择「加载全部菜单」（扫描开始菜单程序）或「不加载菜单」（手动添加）

## 📦 安装

### 方式一：绿色便携版（推荐）

1. 下载 `WinNas-Launcher-portable.zip` 并解压
2. 双击 `winnas-launcher.exe` 运行

便携版**不写注册表、不写 %APPDATA%**，配置/日志保存在程序同目录 `conf/`。已内置 `portable.flag` 启用便携模式。

### 方式二：常规安装版

1. 下载 `WinNas Launcher_x64-setup.exe`
2. 双击安装

安装版配置保存在 `%APPDATA%\WinNasLauncher\conf\`。WebView2 Runtime 缺失时会联网引导下载（Win10/11 通常已内置）。

> 环境要求：Windows 10 / 11（x64），自带 WebView2 Runtime。

## 🕹️ 使用说明

| 按键 | 功能 |
|------|------|
| ↑ ↓ ← → | 移动焦点 / 翻页（最右项按 → 下一页，最左项按 ← 上一页） |
| Enter | 确认 / 启动应用 |
| Esc / 退格 | 返回 / 退出确认 |
| F1 / 菜单键 | 打开设置 |
| 音量 + / - / 静音 | 调节系统音量（OSD 提示） |

**首次启动**：弹窗选择菜单初始化方式 —
- **不加载菜单**：空网格，通过设置 →「添加 APP」手动添加
- **加载全部菜单**：自动扫描开始菜单程序（已过滤卸载/帮助/修复类）

**添加 APP**：设置 →「添加 APP」→ 扫描已安装程序列表选择，或手动输入程序路径（exe/lnk）。

**退出**：首页按 Esc → 确认退出；或设置 →「退出 Launcher」。

**维护模式**：设置 →「进入维护模式」→ 恢复任务栏、Launcher 让位，方便调试 Windows 桌面。

## 🛠️ 开发构建

```bash
# 环境要求
# Node ≥ 20、Rust stable、Tauri v2 CLI
# Windows SDK + VS Build Tools（C++）

# 安装依赖
npm install

# 本地开发（桌面窗口调试）
npm run tauri dev

# 打包（NSIS 安装版 + release exe）
npm run tauri build
```

便携版 zip 由 `target/release/winnas-launcher.exe` + `portable.flag` 手动打包。

## 🏗️ 技术栈

- **前端**：React + TypeScript + Vite
- **壳层**：Tauri v2（WebView2）
- **系统封装**：Rust `windows` crate（Win32 API 封装在 `src-tauri/src/win/`）
  - 窗口全屏置顶、任务栏隐藏、焦点状态机、LL 键盘钩子、Job Object 进程树、Core Audio 音量、图标提取、程序扫描、配置/日志

## 📁 项目结构

```
winnas-launcher/
├── src/                    # React 前端（网格导航、设置抽屉、弹窗、OSD）
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs          # IPC 命令注册 + 启动流程
│   │   └── win/            # Win32 系统封装模块
│   │       ├── window.rs   # 全屏置顶
│   │       ├── focus.rs    # 焦点状态机
│   │       ├── keyhook.rs  # 键盘钩子
│   │       ├── process.rs  # 进程启动/退出检测
│   │       ├── volume.rs   # 音量控制
│   │       ├── scanner.rs  # 程序扫描
│   │       ├── icon.rs     # 图标提取
│   │       ├── config.rs   # 配置/菜单持久化
│   │       ├── system_state.rs  # 系统状态快照自愈
│   │       └── ...
│   └── tauri.conf.json
├── poc/                    # M0 技术验证工程（5 项 PoC）
└── docs/                   # 设计文档、验收清单、打包说明
```

## ❓ 常见问题

**Q：启动程序后窗口不在最前？**
已修复：`.lnk` 启动走 `ShellExecuteExW` + `SEE_MASK_NOCLOSEPROCESS` 拿 PID 并前台激活。

**Q：Alt+Tab 切换其他程序正常吗？**
正常。置顶跟随焦点：Launcher 聚焦时置顶，失焦（切走）时取消置顶，不会挡住其他程序。

**Q：怎么完全重置？**
设置 →「清除桌面菜单缓存」→ 退出后下次启动重新走首次引导。

**Q：日志在哪？**
便携版 `conf/logs/YYYY-MM-DD.log`，安装版 `%APPDATA%\WinNasLauncher\conf\logs\`。

## 📄 许可证

MIT License（可自行修改）。
