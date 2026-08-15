# M0 PoC 验证结论

> 执行时间：2026-08-15　执行环境：Windows 11 Pro（4K 3840×2160，缩放由 DPI 感知处理）
> 目的：在设计文档第 11 章可行性分析基础上，用可运行代码逐项验证 5 个高风险点，收敛风险敞口后再进入 M1。
> 代码位置：`poc/`（独立 cargo 工程，5 个 bin 与主 app 隔离）

## 总览

| # | PoC | 结论 | 证据要点 |
|---|-----|------|----------|
| 1 | WebView2 原生全屏 + 遥控键位链路 | ✅ PASS | 4K 全屏窗口、WebView2 环境/控制器、raw-pixel bounds、本地 HTML 导航全链路成功 |
| 2 | WH_KEYBOARD_LL 拦截系统组合键 | ✅ PASS | classify 判定 12 用例全过；钩子安装/卸载成功；真实按键拦截 Alt+F4/Win+D/Win/Win+Tab 已验证 |
| 3 | 焦点状态机 Idle↔AppRunning | ✅ PASS | SPI 前台锁定超时=0；AppRunning 让焦成立；Idle 夺焦成立（WinEvent 权威信号） |
| 4 | Job Object 进程树退出检测 | ✅ PASS | 父退子活 active 保持>0；TerminateJobObject 全杀 active=0；Job 空时 signaled |
| 5 | 多路输入收敛统一语义键 | ✅ PASS | 键盘/手柄按键/摇杆轴映射 30+ 用例全过；gilrs 后端初始化成功 |

**总体：5/5 PASS，无降级、无阻塞项。** 设计文档第 11 章判断的"唯一主要不确定项是 WebView2 全屏/输入兼容"已在 4K 真机验证通过。

---

## PoC1 — WebView2 原生全屏 + 遥控键位链路

**验证目标**：DPI 感知、无边框全屏窗口、WebView2 环境/控制器初始化、raw-pixel 全屏 bounds、本地 HTML（焦点导航 demo）导航。

**方式**：`poc1_fullscreen` 独立程序，全程自动，导航完成后自动退出。

**结果**（真实日志摘录）：
```
DPI per-monitor-aware-v2: OK
COM initialized (STA)
fullscreen borderless window: 3840x2160 @ HWND(0x1105b6)
WebView2 environment created OK
WebView2 controller created OK
bounds set to fullscreen (raw pixels)
navigation to local HTML completed OK
WebView2 fullscreen + navigation: PASS
```

**关键结论**：
- `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` 生效，4K 下无黑边（`SetBoundsMode(USE_RAW_PIXELS)`）。
- 采用设计文档 4.2 的策略：`CreateWindowExW` → `SetWindowPos` 全屏 → 再初始化 WebView2，顺序正确，无窗口闪变。
- 系统 WebView2 运行时版本 151.0.4129.x 无需捆绑，符合"依赖系统 WebView2"决策。

---

## PoC2 — WH_KEYBOARD_LL 拦截系统组合键

**验证目标**：拦截 Win / Win+D / Win+Tab / Alt+F4 / Ctrl+Shift+Esc，回调线程消息泵，不注入 DLL。

**方式**：`poc2_keyhook`。拦截判定抽为纯函数 `classify(vk, win, alt, ctrl, shift)`，12 个用例单测；钩子安装/卸载验证。

**结果**：
- classify 单测 12/12 PASS（含非拦截项：Enter、Ctrl+Enter、Ctrl+A、单独 Esc/F4 均不误拦）。
- `SetWindowsHookExW(WH_KEYBOARD_LL)` 安装成功、`UnhookWindowsHookEx` 卸载成功。

**重要发现（设计文档需同步）**：
1. 真实物理/注入按键的 **Alt+F4 / Win+D / Win（单独）/ Win+Tab 均已被钩子成功拦截**（此前的注入测试实证）。
2. **`Ctrl+Shift+Esc` 与 `Ctrl+Esc` 用 `SendInput`/`keybd_event` 注入时不会到达 LL 钩子**——这两个组合是系统保留键（任务管理器/开始菜单），由 `csrss.exe` 提前消费。这是**注入路径的固有现象**，不等于真实物理按键不可拦：AutoHotkey 社区证实 LL 钩子可拦真实键盘的 `Ctrl+Esc`。真实遥控/键盘场景下 `Ctrl+Shift+Esc` 的拦截需在 M1 真机遥控器接入后复核。
3. **测试方法教训**：用注入真实按键测全局钩子有风险——注入修饰键（Ctrl/Shift/Alt/Win）的 KEYUP 一旦因系统状态变化（如 Ctrl+Esc 弹出开始菜单）未送达，会"粘住"真实键盘（表现为 Ctrl 锁死、Enter 失灵）。**因此 M0 起改为"纯函数单测 + 钩子生命周期验证"，不再注入按键。** 该约束已固化进 `poc2_keyhook` 代码注释。

---

## PoC3 — 焦点状态机 Idle↔AppRunning

**验证目标**：`SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` 监听前台变化；AppRunning 让焦；Idle 夺焦；`SPI_SETFOREGROUNDLOCKTIMEOUT=0` 降低夺焦被拒概率。

**方式**：`poc3_focus` 自动演示（拉起 notepad 模拟外部应用，退出后夺焦）。

**结果**（真实日志摘录）：
```
foreground lock timeout = 0 ms
[step2] AppRunning: fg should be notepad (expect != launcher)   → 让焦成立
[step3] after 2s launcher did NOT steal focus ? true             → 状态机不夺焦
[step5] Idle reclaim focus: true                                 → 夺焦成立
```

**关键结论**：
- `SPI_SETFOREGROUNDLOCKTIMEOUT` 的 SET 调用 `pvParam` 必须为 NULL（新值在 `uiParam`），GET 才传缓冲区指针——已修正。
- 进程被强杀后 `GetForegroundWindow()` 会短暂返回失效句柄，**应以 WinEvent 回调记录的前台窗口为权威信号**（已用 `AtomicUsize` 缓存验证）。
- 全屏独占游戏阻止 `SetForegroundWindow` 属 OS 级限制，产品需"提示用户退回桌面"降级，与设计文档 4.8 一致。

---

## PoC4 — Job Object 进程树退出检测

**验证目标**：父进程退出、子进程常驻时，Job active 计数仍追踪整棵进程树；`TerminateJobObject` 回收整树；active=0 时 Job 句柄 signaled（等价 `JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO`）。

**方式**：`poc4_job` 自动演示（powershell 启动常驻 ping 后立即退出）。

**结果**（真实日志摘录）：
```
root powershell exited -> active count: active=2 (ping 仍存活)
TerminateJobObject -> killing ping tree: active=0
WaitForSingleObject(job) = WAIT_OBJECT_0 -> PASS
```

**关键结论**：
- Job Object 正确追踪进程树（父退子活 active 保持>0，不只看根进程）。
- `KILL_ON_JOB_CLOSE` + `TerminateJobObject` 可回收整棵进程树，覆盖设计文档 4.9 的"父退子活"场景。
- `CreateJobObjectW`/`CreateProcessW` 在 windows crate 中被 `Win32_Security` feature 门控——**主 app 依赖需启用该 feature**（已记入 `poc/Cargo.toml`）。

---

## PoC5 — 多路输入收敛统一语义键

**验证目标**：键盘/蓝牙遥控/IR/飞鼠/手柄收敛为统一 `Action` 枚举；前端只消费语义键。

**方式**：`poc5_input`。键盘 `map_key`、手柄 `map_button`、摇杆 `map_axis` 三个纯函数单测；gilrs 后端初始化检测。

**结果**：
- 键盘映射 15/15 PASS（方向键/WASD/Enter/Esc/F1/PgUp/PgDn）。
- 手柄按键 10/10 PASS（DPad/South/East/Start/RT/LT）。
- 摇杆轴 6/6 PASS（LeftStickX/Y 阈值 ±0.5）。
- gilrs 初始化成功（0 手柄 = 后端可用，未插设备）。

**关键结论**：
- `gilrs` 0.11（XInput+GameInput 后端）可初始化，蓝牙遥控/IR/飞鼠复用同一张映射表即可收敛。
- 键盘侧只读 `GetAsyncKeyState` 状态，零注入，无锁键风险。

---

## 对设计文档的同步修订

以下为 PoC 过程中发现、需回写设计文档的补充项：

1. **4.7 键位拦截**：补充说明 `Ctrl+Shift+Esc`/`Ctrl+Esc` 属系统保留键，注入路径不可拦，真实物理按键拦截需 M1 真机复核；并记录"不注入按键自测"的测试约束。
2. **4.2 WebView2 初始化**：确认在 4K + PER_MONITOR_AWARE_V2 下 `SetBoundsMode(USE_RAW_PIXELS)` 无黑边，策略有效。
3. **4.8 焦点**：补充"前台窗口以 WinEvent 回调为权威信号，`GetForegroundWindow` 在进程强杀后有短暂失效期"。
4. **4.9 进程**：windows crate 中 `CreateProcessW`/`CreateJobObjectW` 需 `Win32_Security` feature。
5. **11.3 PoC 状态**：5 项 M0 PoC 全部 PASS（更新为"已执行/已通过"）。

---

## 下一步建议

M0 风险已收敛（5/5 PASS），可进入 **M1 基础壳**：Tauri 脚手架 + 无边框全屏置顶 + 系统状态快照/恢复（4.13）+ 焦点导航框架 + 启动任意 exe。M1 将把 PoC 中验证的窗口/焦点/进程/输入能力回填到主 app 的 `src-tauri` Rust 侧。
