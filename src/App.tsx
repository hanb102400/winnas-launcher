import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { t, getLang, setLang, LANGS } from "./i18n";
import "./App.css";

// 网格菜单应用项（对应 Rust config::AppItem）
interface AppItem {
  name: string;
  exe: string;
  icon: string;
}

interface Config {
  autostart: boolean;
  exit_confirm: boolean;
  scale: number;
  initialized: boolean;
  menu_mode: string;
  language: string;
}

// 网格规模（设计文档 5.2：6 列 × 4 行 = 24 项/页）
const COLS = 6;
const ROWS = 4;
const PAGE_SIZE = COLS * ROWS;

function App() {
  const [apps, setApps] = useState<AppItem[]>([]);
  const [focusIndex, setFocusIndex] = useState(0);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [toast, setToast] = useState("");
  const [autostart, setAutostart] = useState(false);
  const [confirmExit, setConfirmExit] = useState(false);
  const [confirmFocus, setConfirmFocus] = useState(0); // 0=确认 1=取消（退出/系统操作确认框共用）
  const [gearFocused, setGearFocused] = useState(false);
  const [volume, setVolume] = useState(0);
  const [osdVisible, setOsdVisible] = useState(false);
  const [systemAction, setSystemAction] = useState<string | null>(null);
  const [showKeymap, setShowKeymap] = useState(false);
  const [langPickerOpen, setLangPickerOpen] = useState(false);
  const [langPickerFocus, setLangPickerFocus] = useState(0); // 0..9=语言项, 10=返回
  const [manageMode, setManageMode] = useState(false);
  const [manageTarget, setManageTarget] = useState<AppItem | null>(null);
  const [manageAction, setManageAction] = useState(0); // 0=删除 1=改名 2=移到最前 3=移到最后 4=取消
  const [renaming, setRenaming] = useState(false);
  const [renameTarget, setRenameTarget] = useState<AppItem | null>(null);
  const [renameInput, setRenameInput] = useState("");
  const [renameFocus, setRenameFocus] = useState(0); // 0=输入框 1=确认 2=取消
  const [maintenance, setMaintenance] = useState(false);
  const backDown = useRef(false);
  const menuDown = useRef(false);
  const longPressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const renameInputRef = useRef<HTMLInputElement>(null);
  const manualInputRef = useRef<HTMLInputElement>(null);
  const langListRef = useRef<HTMLDivElement>(null); // 语言弹窗滚动列表
  const scanListRef = useRef<HTMLDivElement>(null); // 添加 APP 扫描列表
  const settingsDrawerRef = useRef<HTMLElement>(null); // 设置抽屉滚动列表
  const [settingsFocus, setSettingsFocus] = useState(0); // 设置抽屉内焦点索引
  const [clock, setClock] = useState(() => new Date());
  const [firstRun, setFirstRun] = useState(false); // 首次启动引导
  const [firstRunFocus, setFirstRunFocus] = useState(0); // 0=不加载 1=加载全部
  const [showAddApp, setShowAddApp] = useState(false);
  const [scanList, setScanList] = useState<AppItem[]>([]);
  // 添加 APP 页统一焦点索引：0..len-1=列表项，len=输入框，len+1=添加，len+2=浏览文件，len+3=返回
  const [addAppFocus, setAddAppFocus] = useState(0);
  const [manualPath, setManualPath] = useState("");

  // 分页派生状态（设计文档 5.3 网格分页导航）
  const pageCount = Math.max(1, Math.ceil(apps.length / PAGE_SIZE));
  const pageIndex = Math.min(Math.floor(focusIndex / PAGE_SIZE), pageCount - 1);
  const inPage = focusIndex % PAGE_SIZE;
  const row = Math.floor(inPage / COLS);
  const col = inPage % COLS;
  const pageApps = apps.slice(pageIndex * PAGE_SIZE, (pageIndex + 1) * PAGE_SIZE);

  // 系统操作确认框打开时，焦点重置到「确认」
  useEffect(() => {
    if (systemAction) setConfirmFocus(0);
  }, [systemAction]);

  // 时钟
  useEffect(() => {
    const t = setInterval(() => setClock(new Date()), 1000);
    return () => clearInterval(t);
  }, []);

  // 监听音量变化（keyhook 转发）→ 显示 OSD，防抖 1.5s 渐隐
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout>;
    const unlisten = listen<number>("volume-changed", (e) => {
      setVolume(e.payload);
      setOsdVisible(true);
      clearTimeout(timer);
      timer = setTimeout(() => setOsdVisible(false), 1500);
    });
    return () => {
      unlisten.then((f) => f());
      clearTimeout(timer);
    };
  }, []);

  // 挂载：读配置 + 菜单列表，判断是否首次启动
  useEffect(() => {
    (async () => {
      const config = await invoke<Config>("get_config");
      // 应用界面语言（config.language 首次启动已由 Rust 按系统语言检测写回）
      const lang = config.language || "zh-CN";
      setLang(lang);
      document.documentElement.dir = lang === "ar" ? "rtl" : "ltr";
      setAutostart(config.autostart);
      if (!config.initialized) {
        setFirstRun(true);
      }
      const list = await invoke<AppItem[]>("get_apps");
      setApps(list);
    })().catch(() => {});
  }, []);

  // 键盘/遥控器滚动：焦点超出列表可视区时自动滚入（遥控器没有滚轮）
  // 依赖列表容器 position:relative，使 item.offsetTop 相对容器顶部，与 scrollTop 一致。
  const scrollFocusedIntoView = useCallback((container: HTMLElement | null, selector: string) => {
    if (!container) return;
    const el = container.querySelector<HTMLElement>(selector);
    if (!el) return;
    const itemTop = el.offsetTop;
    const itemBottom = itemTop + el.offsetHeight;
    const visibleBottom = container.scrollTop + container.clientHeight;
    if (itemBottom > visibleBottom) {
      container.scrollTop = itemBottom - container.clientHeight;
    } else if (itemTop < container.scrollTop) {
      container.scrollTop = itemTop;
    }
  }, []);

  useEffect(() => {
    if (langPickerOpen) scrollFocusedIntoView(langListRef.current, ".lang-item.focused");
  }, [langPickerOpen, langPickerFocus, scrollFocusedIntoView]);

  useEffect(() => {
    if (showAddApp) scrollFocusedIntoView(scanListRef.current, ".scan-item.focused");
  }, [showAddApp, addAppFocus, scrollFocusedIntoView]);

  useEffect(() => {
    if (settingsOpen) scrollFocusedIntoView(settingsDrawerRef.current, ".drawer-item.focused");
  }, [settingsOpen, settingsFocus, scrollFocusedIntoView]);

  const launch = useCallback(async (app: AppItem) => {
    setToast(t("toastLaunching", { name: app.name }));
    try {
      const pid = await invoke<number>("launch_app", { path: app.exe });
      setToast(
        pid > 0
          ? t("toastLaunchedPid", { name: app.name, pid })
          : t("toastLaunched", { name: app.name }),
      );
    } catch (e) {
      setToast(t("toastLaunchFailed", { err: String(e) }));
    }
  }, []);

  const restoreDesktop = useCallback(async () => {
    await invoke("restore_desktop");
    setToast(t("toastRestored"));
    setSettingsOpen(false);
  }, []);

  // 清除菜单缓存：清空 apps + initialized=false，退出后重新走首次引导
  const clearMenuCache = useCallback(async () => {
    await invoke("clear_menu_cache");
    setSettingsOpen(false);
    setConfirmExit(true);
    setConfirmFocus(0);
  }, []);

  // 进入管理模式（焦点回到网格）
  const enterManageMode = useCallback(() => {
    setSettingsOpen(false);
    setManageMode(true);
    setManageTarget(null);
    setFocusIndex(0);
  }, []);

  // 退出管理模式（焦点回到设置）
  const exitManageMode = useCallback(() => {
    setManageMode(false);
    setManageTarget(null);
    setSettingsOpen(true);
    setSettingsFocus(0);
  }, []);

  // 执行管理操作（删除/改名/移到最前/移到最后/取消）
  const executeManageAction = useCallback(async () => {
    if (!manageTarget) return;
    const exe = manageTarget.exe;
    try {
      let list: AppItem[];
      switch (manageAction) {
        case 0:
          list = await invoke<AppItem[]>("remove_app", { exe });
          setToast(t("toastDeleted", { name: manageTarget.name }));
          break;
        case 1:
          // 改名：关闭操作菜单，进入改名输入状态
          setRenameTarget(manageTarget);
          setRenameInput(manageTarget.name);
          setManageTarget(null);
          setRenaming(true);
          return;
        case 2:
          list = await invoke<AppItem[]>("move_app_to_front", { exe });
          setToast(t("toastMovedFront", { name: manageTarget.name }));
          break;
        case 3:
          list = await invoke<AppItem[]>("move_app_to_end", { exe });
          setToast(t("toastMovedEnd", { name: manageTarget.name }));
          break;
        default:
          list = apps;
      }
      setApps(list);
      // 删除收缩列表后夹紧焦点，避免 focusIndex 越界访问 undefined
      setFocusIndex((f) => Math.min(f, Math.max(0, list.length - 1)));
    } catch (e) {
      setToast(t("toastOpFailed", { err: String(e) }));
    }
    setManageTarget(null);
  }, [manageTarget, manageAction, apps]);

  // 确认改名
  const confirmRename = useCallback(async () => {
    if (!renameTarget) return;
    const name = renameInput.trim();
    if (!name) return;
    try {
      const list = await invoke<AppItem[]>("rename_app", { exe: renameTarget.exe, name });
      setApps(list);
      setToast(t("toastRenamed", { name }));
    } catch (e) {
      setToast(t("toastRenameFailed", { err: String(e) }));
    }
    setRenaming(false);
    setRenameTarget(null);
  }, [renameTarget, renameInput]);

  // 进入/退出维护模式（长按 Back+Menu 触发，见设计文档 6.1）
  const toggleMaintenance = useCallback(async () => {
    if (maintenance) {
      await invoke("exit_maintenance");
      setMaintenance(false);
      setToast(t("toastExitMaintenance"));
    } else {
      await invoke("enter_maintenance");
      setMaintenance(true);
      setToast(t("toastEnterMaintenance"));
    }
  }, [maintenance]);

  // 长按「返回 + 菜单」5 秒进入/退出维护模式
  useEffect(() => {
    const isBack = (k: string) => k === "Escape" || k === "Backspace" || k === "BrowserBack";
    const isMenu = (k: string) => k === "F1" || k === "ContextMenu";

    const down = (e: KeyboardEvent) => {
      if (isBack(e.key)) backDown.current = true;
      if (isMenu(e.key)) menuDown.current = true;
      if (backDown.current && menuDown.current && !longPressTimer.current) {
        longPressTimer.current = setTimeout(() => {
          if (backDown.current && menuDown.current) {
            toggleMaintenance();
          }
          longPressTimer.current = null;
        }, 5000);
      }
    };
    const up = (e: KeyboardEvent) => {
      if (isBack(e.key)) backDown.current = false;
      if (isMenu(e.key)) menuDown.current = false;
      if (!(backDown.current && menuDown.current) && longPressTimer.current) {
        clearTimeout(longPressTimer.current);
        longPressTimer.current = null;
      }
    };

    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
      if (longPressTimer.current) clearTimeout(longPressTimer.current);
    };
  }, [toggleMaintenance]);

  const toggleAutostart = useCallback(async () => {
    const next = !autostart;
    const ok = await invoke<boolean>("set_autostart", { enabled: next });
    if (ok) setAutostart(next);
    setToast(
      ok ? (next ? t("toastAutostartOn") : t("toastAutostartOff")) : t("toastAutostartFailed"),
    );
  }, [autostart]);

  const doExit = useCallback(() => {
    invoke("exit_app");
  }, []);

  // 首次启动引导选择
  const chooseMenu = useCallback(async (mode: string) => {
    setToast(mode === "all" ? t("toastScanning") : t("toastNoMenu"));
    try {
      const list = await invoke<AppItem[]>("init_menu", { mode });
      setApps(list);
      setFirstRun(false);
      setFocusIndex(0);
      setToast(mode === "all" ? t("toastLoaded", { count: list.length }) : t("toastEmptyMenu"));
    } catch (e) {
      setToast(t("toastInitFailed", { err: String(e) }));
    }
  }, []);

  // 切换语言：持久化到 setting.conf 后整页重载应用新语言（FR-20 / 设计文档 5.9）
  const changeLanguage = useCallback(async (code: string) => {
    try {
      const ok = await invoke<boolean>("set_language", { code });
      if (!ok) {
        setToast(t("toastOpFailed", { err: code }));
        return;
      }
    } catch (e) {
      setToast(t("toastOpFailed", { err: String(e) }));
      return;
    }
    setLang(code);
    setLangPickerOpen(false);
    window.location.reload();
  }, []);

  // 打开添加 APP 页并扫描已安装程序
  const openAddApp = useCallback(async () => {
    setShowAddApp(true);
    setSettingsOpen(false);
    setAddAppFocus(0);
    setScanList([]);
    setToast(t("toastScanningInstalled"));
    try {
      const list = await invoke<AppItem[]>("scan_apps");
      setScanList(list);
      setToast("");
    } catch (e) {
      setToast(t("toastScanFailed", { err: String(e) }));
    }
  }, []);

  // 添加应用（扫描列表选中）
  const addAppToGrid = useCallback(async (app: AppItem) => {
    try {
      const list = await invoke<AppItem[]>("add_app", { app });
      setApps(list);
      setShowAddApp(false);
      setFocusIndex(0);
      setToast(t("toastAdded", { name: app.name }));
    } catch (e) {
      setToast(t("toastAddFailed", { err: String(e) }));
    }
  }, []);

  // 手动输入路径添加
  const addManualApp = useCallback(async () => {
    const p = manualPath.trim();
    if (!p) {
      setToast(t("toastEnterPath"));
      return;
    }
    const name = p.split(/[\\/]/).pop()?.replace(/\.(exe|lnk)$/i, "") || p;
    await addAppToGrid({ name, exe: p, icon: "" });
    setManualPath("");
  }, [manualPath, addAppToGrid]);

  // 浏览文件选择应用（原生对话框）
  const browseFile = useCallback(async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: t("fileFilterApps"), extensions: ["exe", "lnk"] }],
      });
      if (selected && typeof selected === "string") {
        const name = selected.split(/[\\/]/).pop()?.replace(/\.(exe|lnk)$/i, "") || selected;
        await addAppToGrid({ name, exe: selected, icon: "" });
      }
    } catch (e) {
      setToast(t("toastBrowseFailed", { err: String(e) }));
    }
  }, [addAppToGrid]);

  // 执行设置抽屉第 idx 项
  const executeSettings = useCallback(
    (idx: number) => {
      switch (idx) {
        case 0: {
          // 语言（焦点默认落在当前语言项）
          const cur = LANGS.findIndex((l) => l.code === getLang());
          setLangPickerFocus(cur >= 0 ? cur : 0);
          setLangPickerOpen(true);
          break;
        }
        case 1:
          restoreDesktop();
          break;
        case 2:
          clearMenuCache();
          break;
        case 3:
          toggleAutostart();
          break;
        case 4:
          openAddApp();
          break;
        case 5:
          enterManageMode();
          break;
        case 6:
          setShowKeymap(true);
          break;
        case 7:
          setSystemAction("reboot");
          break;
        case 8:
          setSystemAction("sleep");
          break;
        case 9:
          setSystemAction("lock");
          break;
        case 10:
          setSettingsOpen(false);
          setConfirmExit(true);
          setConfirmFocus(0);
          break;
        case 11:
          setSettingsOpen(false);
          break;
      }
    },
    [restoreDesktop, clearMenuCache, toggleAutostart, openAddApp, enterManageMode],
  );

  // 确认系统操作
  const confirmSystemAction = useCallback(async () => {
    if (!systemAction) return;
    const cmd = {
      shutdown: "system_shutdown",
      reboot: "system_reboot",
      sleep: "system_sleep",
      lock: "system_lock",
    }[systemAction] ?? "";
    await invoke(cmd);
    setSystemAction(null);
    setSettingsOpen(false);
  }, [systemAction]);

  const actionLabel = (a: string) =>
    (
      {
        shutdown: t("actionShutdown"),
        reboot: t("actionReboot"),
        sleep: t("actionSleep"),
        lock: t("actionLock"),
      } as Record<string, string>
    )[a] || a;

  // 遥控/键盘焦点导航（二维网格 + 分页翻页，见设计文档 5.3）
  const onKeyDown = useCallback(
    (e: KeyboardEvent) => {
      // 长按 Back+Menu 期间，跳过单按处理（等长按计时器触发维护模式）
      if (backDown.current && menuDown.current) {
        e.preventDefault();
        return;
      }

      // 首次启动引导窗：左右切换，Enter 确认
      if (firstRun) {
        switch (e.key) {
          case "ArrowLeft":
          case "ArrowRight":
          case "ArrowUp":
          case "ArrowDown":
            setFirstRunFocus((f) => (f + 1) % 2);
            break;
          case "Enter":
            chooseMenu(firstRunFocus === 0 ? "manual" : "all");
            break;
        }
        e.preventDefault();
        return;
      }

      // 退出确认框打开时：左右切换焦点，Enter 执行当前，Esc/Backspace 取消
      if (confirmExit) {
        switch (e.key) {
          case "ArrowLeft":
          case "ArrowRight":
          case "ArrowUp":
          case "ArrowDown":
            setConfirmFocus((f) => (f + 1) % 2);
            break;
          case "Enter":
            if (confirmFocus === 0) {
              doExit();
            } else {
              setConfirmExit(false);
            }
            break;
          case "Escape":
          case "Backspace":
          case "BrowserBack":
            setConfirmExit(false);
            break;
        }
        e.preventDefault();
        return;
      }

      // 系统操作确认框：左右切换焦点，Enter 执行当前，Esc/Backspace 取消
      if (systemAction) {
        switch (e.key) {
          case "ArrowLeft":
          case "ArrowRight":
          case "ArrowUp":
          case "ArrowDown":
            setConfirmFocus((f) => (f + 1) % 2);
            break;
          case "Enter":
            if (confirmFocus === 0) {
              confirmSystemAction();
            } else {
              setSystemAction(null);
            }
            break;
          case "Escape":
          case "Backspace":
          case "BrowserBack":
            setSystemAction(null);
            break;
        }
        e.preventDefault();
        return;
      }

      // 按键说明弹窗：任意键关闭
      if (showKeymap) {
        if (e.key === "Escape" || e.key === "Backspace" || e.key === "BrowserBack" || e.key === "Enter") {
          setShowKeymap(false);
        }
        e.preventDefault();
        return;
      }

      // 改名输入框：方向键在输入框/确认/取消间导航，Enter 确认，Esc 取消
      if (renaming) {
        switch (e.key) {
          case "ArrowDown":
            setRenameFocus((f) => Math.min(f + 1, 2));
            break;
          case "ArrowUp":
            setRenameFocus((f) => {
              const next = Math.max(f - 1, 0);
              if (next === 0) renameInputRef.current?.focus();
              return next;
            });
            break;
          case "Enter":
            if (renameFocus === 2) {
              setRenaming(false);
            } else {
              confirmRename();
            }
            break;
          case "Escape":
          case "Backspace":
          case "BrowserBack":
            setRenaming(false);
            break;
        }
        e.preventDefault();
        return;
      }

      // 管理 APP 操作菜单：方向键选择，Enter 执行，Esc 取消
      if (manageTarget) {
        switch (e.key) {
          case "ArrowDown":
            setManageAction((f) => Math.min(f + 1, 4));
            break;
          case "ArrowUp":
            setManageAction((f) => Math.max(f - 1, 0));
            break;
          case "Enter":
            executeManageAction();
            break;
          case "Escape":
          case "Backspace":
          case "BrowserBack":
            setManageTarget(null);
            break;
        }
        e.preventDefault();
        return;
      }

      // 添加 APP 页：统一焦点（列表项 → 输入框 → 添加 → 浏览文件 → 返回），Enter 执行，Esc 返回
      if (showAddApp) {
        const inputIdx = scanList.length;
        const addIdx = scanList.length + 1;
        const browseIdx = scanList.length + 2;
        const backIdx = scanList.length + 3;
        switch (e.key) {
          case "ArrowDown":
            setAddAppFocus((f) => {
              const next = Math.min(f + 1, backIdx);
              if (next === inputIdx) manualInputRef.current?.focus();
              return next;
            });
            break;
          case "ArrowUp":
            setAddAppFocus((f) => {
              const next = Math.max(f - 1, 0);
              if (next === inputIdx) manualInputRef.current?.focus();
              return next;
            });
            break;
          case "Enter":
            if (addAppFocus < inputIdx) {
              addAppToGrid(scanList[addAppFocus]);
            } else if (addAppFocus === inputIdx) {
              manualInputRef.current?.focus();
            } else if (addAppFocus === addIdx) {
              addManualApp();
            } else if (addAppFocus === browseIdx) {
              browseFile();
            } else {
              setShowAddApp(false);
            }
            break;
          case "Escape":
          case "Backspace":
          case "BrowserBack":
            setShowAddApp(false);
            break;
        }
        e.preventDefault();
        return;
      }

      // 语言选择弹窗：上下选择（0..9=语言项，10=返回），Enter 执行，Esc 关闭
      if (langPickerOpen) {
        const LAST = LANGS.length; // 返回项索引
        switch (e.key) {
          case "ArrowDown":
            setLangPickerFocus((f) => Math.min(f + 1, LAST));
            break;
          case "ArrowUp":
            setLangPickerFocus((f) => Math.max(f - 1, 0));
            break;
          case "Enter":
            if (langPickerFocus < LAST) {
              changeLanguage(LANGS[langPickerFocus].code);
            } else {
              setLangPickerOpen(false);
            }
            break;
          case "Escape":
          case "Backspace":
          case "BrowserBack":
            setLangPickerOpen(false);
            break;
        }
        e.preventDefault();
        return;
      }

      // 设置抽屉打开：上下选择，Enter 执行，Esc 关闭
      if (settingsOpen) {
        switch (e.key) {
          case "ArrowDown":
            setSettingsFocus((f) => Math.min(f + 1, 11));
            break;
          case "ArrowUp":
            setSettingsFocus((f) => Math.max(f - 1, 0));
            break;
          case "Enter":
            executeSettings(settingsFocus);
            break;
          case "Escape":
          case "Backspace":
          case "BrowserBack":
            setSettingsOpen(false);
            break;
        }
        e.preventDefault();
        return;
      }

      // 设置齿轮获得焦点时的导航
      if (gearFocused) {
        switch (e.key) {
          case "ArrowDown":
          case "ArrowLeft":
            setGearFocused(false); // 回到网格
            break;
          case "Enter":
            setSettingsOpen(true);
            setSettingsFocus(0);
            setGearFocused(false); // 焦点进入抽屉
            break;
          case "F1":
          case "ContextMenu":
            setSettingsOpen((o) => !o);
            setSettingsFocus(0);
            break;
          case "Escape":
          case "Backspace":
          case "BrowserBack":
            setGearFocused(false);
            break;
        }
        e.preventDefault();
        return;
      }

      // 空网格：方向键移到设置齿轮，F1 打开设置，Esc 退出
      if (apps.length === 0) {
        if (e.key === "ArrowUp" || e.key === "ArrowDown" || e.key === "ArrowLeft" || e.key === "ArrowRight") {
          setGearFocused(true);
        } else if (e.key === "F1" || e.key === "ContextMenu") {
          setSettingsOpen((o) => !o);
          setSettingsFocus(0);
        } else if (e.key === "Escape" || e.key === "Backspace" || e.key === "BrowserBack") {
          if (manageMode) {
            exitManageMode();
          } else {
            setConfirmExit(true);
            setConfirmFocus(0);
          }
        }
        e.preventDefault();
        return;
      }

      switch (e.key) {
        case "ArrowRight":
          if (col < COLS - 1) {
            // 页内右移
            if (focusIndex + 1 < apps.length) setFocusIndex(focusIndex + 1);
          } else if (pageIndex < pageCount - 1) {
            // 最右列 → 下一页同行的最左列
            const target = (pageIndex + 1) * PAGE_SIZE + row * COLS;
            setFocusIndex(Math.min(target, apps.length - 1));
          }
          e.preventDefault();
          break;
        case "ArrowLeft":
          if (col > 0) {
            setFocusIndex(focusIndex - 1);
          } else if (pageIndex > 0) {
            // 最左列 → 上一页同行的最右列
            const target = (pageIndex - 1) * PAGE_SIZE + row * COLS + (COLS - 1);
            setFocusIndex(Math.min(target, apps.length - 1));
          }
          e.preventDefault();
          break;
        case "PageDown":
          // 下一页（保持同一行/列位置），末页无操作
          if (pageIndex < pageCount - 1) {
            const target = (pageIndex + 1) * PAGE_SIZE + row * COLS + col;
            setFocusIndex(Math.min(target, apps.length - 1));
          }
          e.preventDefault();
          break;
        case "PageUp":
          // 上一页（保持同一行/列位置），首页无操作
          if (pageIndex > 0) {
            const target = (pageIndex - 1) * PAGE_SIZE + row * COLS + col;
            setFocusIndex(Math.min(target, apps.length - 1));
          }
          e.preventDefault();
          break;
        case "ArrowUp":
          if (row > 0) {
            setFocusIndex(focusIndex - COLS);
          } else {
            // 网格第一行按上 → 焦点移到设置齿轮
            setGearFocused(true);
          }
          e.preventDefault();
          break;
        case "ArrowDown":
          if (row < ROWS - 1 && focusIndex + COLS < apps.length) {
            setFocusIndex(focusIndex + COLS);
          }
          e.preventDefault();
          break;
        case "Enter":
          if (manageMode) {
            // 管理模式：弹出操作菜单（而非启动）
            if (apps.length > 0) {
              setManageTarget(apps[focusIndex]);
              setManageAction(0);
            }
          } else {
            launch(apps[focusIndex]);
          }
          e.preventDefault();
          break;
        case "Escape":
        case "Backspace":
        case "BrowserBack":
          if (manageMode) {
            exitManageMode();
          } else {
            setConfirmExit(true);
            setConfirmFocus(0);
          }
          e.preventDefault();
          break;
        case "F1":
        case "ContextMenu":
          setSettingsOpen((o) => !o);
          setSettingsFocus(0);
          e.preventDefault();
          break;
      }
    },
    [
      firstRun,
      firstRunFocus,
      chooseMenu,
      confirmExit,
      confirmFocus,
      systemAction,
      confirmSystemAction,
      showKeymap,
      langPickerOpen,
      langPickerFocus,
      changeLanguage,
      showAddApp,
      scanList,
      addAppFocus,
      addAppToGrid,
      browseFile,
      addManualApp,
      manageMode,
      manageTarget,
      manageAction,
      executeManageAction,
      renaming,
      renameInput,
      confirmRename,
      exitManageMode,
      settingsOpen,
      settingsFocus,
      executeSettings,
      gearFocused,
      apps,
      focusIndex,
      pageIndex,
      pageCount,
      row,
      col,
      launch,
      doExit,
    ],
  );

  useEffect(() => {
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onKeyDown]);

  // 全局禁用右键菜单：宿主级 WebView2 菜单已由 Rust 关闭，这里拦截 DOM 级
  // contextmenu，兜底宿主设置生效前的空窗期与纯 DOM 触发的菜单
  useEffect(() => {
    const blockContextMenu = (e: MouseEvent) => e.preventDefault();
    document.addEventListener("contextmenu", blockContextMenu, true);
    return () =>
      document.removeEventListener("contextmenu", blockContextMenu, true);
  }, []);

  const hh = String(clock.getHours()).padStart(2, "0");
  const mm = String(clock.getMinutes()).padStart(2, "0");

  // 按键说明表（随语言重渲染）
  const KEYMAP: { key: string; action: string }[] = [
    { key: "↑ ↓ ← →", action: t("keyMove") },
    { key: "Page Up / Page Down", action: t("keyPage") },
    { key: "Enter", action: t("keyConfirm") },
    { key: "Esc / 退格", action: t("keyBack") },
    { key: "F1 / 菜单键", action: t("keySettings") },
    { key: "音量 + / -", action: t("keyVolume") },
    { key: "静音键", action: t("keyMute") },
  ];

  return (
    <div className="launcher">
      {/* 顶部信息栏 */}
      <header className="topbar">
        <div className="brand">
          <span className="logo">🖥️</span>
          <span className="title">WinNas Launcher</span>
        </div>
        <div className="top-right">
          <span className="clock">
            {hh}:{mm}
          </span>
          <button
            className={`gear ${settingsOpen ? "active" : ""} ${gearFocused ? "focused" : ""}`}
            onClick={() => setSettingsOpen((o) => !o)}
            onMouseEnter={() => setGearFocused(true)}
          >
            ⚙
          </button>
        </div>
      </header>

      {/* 中央网格菜单 */}
      <div className="stage">
        {apps.length === 0 ? (
          <div className="empty-hint">{t("emptyHint")}</div>
        ) : (
          <div className="grid">
            {pageApps.map((app, i) => {
              const focused = focusIndex === pageIndex * PAGE_SIZE + i;
              return (
                <button
                  key={`${app.exe}-${i}`}
                  className={`tile ${focused ? "focused" : ""} ${manageMode ? "managing" : ""}`}
                  onClick={() => {
                    setFocusIndex(pageIndex * PAGE_SIZE + i);
                    launch(app);
                  }}
                  onMouseEnter={() => setFocusIndex(pageIndex * PAGE_SIZE + i)}
                >
                  {app.icon.startsWith("data:") ? (
                    <img className="tile-icon-img" src={app.icon} alt="" />
                  ) : (
                    <span className="tile-icon">{app.icon || "📦"}</span>
                  )}
                  <span className="tile-name">{app.name}</span>
                </button>
              );
            })}
          </div>
        )}
      </div>

      {/* 分页指示器 */}
      {apps.length > PAGE_SIZE && (
        <div className="pagination">
          {Array.from({ length: pageCount }).map((_, i) => (
            <span key={i} className={`dot ${i === pageIndex ? "active" : ""}`} />
          ))}
        </div>
      )}

      {/* 提示条 */}
      <div className="hint">
        {maintenance ? t("hintMaintenance") : manageMode ? t("hintManage") : t("hintNormal")}
      </div>

      {/* Toast */}
      {toast && <div className="toast">{toast}</div>}

      {/* 音量 OSD */}
      {osdVisible && (
        <div className="osd">
          <span className="osd-icon">🔊</span>
          <div className="osd-bar">
            <div className="osd-fill" style={{ width: `${volume * 100}%` }} />
          </div>
          <span className="osd-text">{Math.round(volume * 100)}%</span>
        </div>
      )}

      {/* 右侧设置抽屉 */}
      {settingsOpen && (
        <aside className="drawer" ref={settingsDrawerRef}>
          <h2>{t("settings")}</h2>
          <button
            className={`drawer-item ${settingsFocus === 0 ? "focused" : ""}`}
            onClick={() => {
              const cur = LANGS.findIndex((l) => l.code === getLang());
              setLangPickerFocus(cur >= 0 ? cur : 0);
              setLangPickerOpen(true);
            }}
            onMouseEnter={() => setSettingsFocus(0)}
          >
            {t("languageEntry")}：{LANGS.find((l) => l.code === getLang())?.native ?? ""}
          </button>
          <button
            className={`drawer-item ${settingsFocus === 1 ? "focused" : ""}`}
            onClick={restoreDesktop}
            onMouseEnter={() => setSettingsFocus(1)}
          >
            {t("restoreDesktop")}
          </button>
          <button
            className={`drawer-item ${settingsFocus === 2 ? "focused" : ""}`}
            onClick={clearMenuCache}
            onMouseEnter={() => setSettingsFocus(2)}
          >
            {t("clearMenuCache")}
          </button>
          <button
            className={`drawer-item ${settingsFocus === 3 ? "focused" : ""}`}
            onClick={toggleAutostart}
            onMouseEnter={() => setSettingsFocus(3)}
          >
            {t("autostart", { state: autostart ? t("autostartOn") : t("autostartOff") })}
          </button>
          <button
            className={`drawer-item ${settingsFocus === 4 ? "focused" : ""}`}
            onClick={openAddApp}
            onMouseEnter={() => setSettingsFocus(4)}
          >
            {t("addApp")}
          </button>
          <button
            className={`drawer-item ${settingsFocus === 5 ? "focused" : ""}`}
            onClick={enterManageMode}
            onMouseEnter={() => setSettingsFocus(5)}
          >
            {t("manageApp")}
          </button>
          <button
            className={`drawer-item ${settingsFocus === 6 ? "focused" : ""}`}
            onClick={() => setShowKeymap(true)}
            onMouseEnter={() => setSettingsFocus(6)}
          >
            {t("keymapEntry")}
          </button>
          <button
            className={`drawer-item ${settingsFocus === 7 ? "focused" : ""}`}
            onClick={() => setSystemAction("reboot")}
            onMouseEnter={() => setSettingsFocus(7)}
          >
            {t("reboot")}
          </button>
          <button
            className={`drawer-item ${settingsFocus === 8 ? "focused" : ""}`}
            onClick={() => setSystemAction("sleep")}
            onMouseEnter={() => setSettingsFocus(8)}
          >
            {t("sleep")}
          </button>
          <button
            className={`drawer-item ${settingsFocus === 9 ? "focused" : ""}`}
            onClick={() => setSystemAction("lock")}
            onMouseEnter={() => setSettingsFocus(9)}
          >
            {t("lock")}
          </button>
          <button
            className={`drawer-item ${settingsFocus === 10 ? "focused" : ""}`}
            onClick={() => {
              setSettingsOpen(false);
              setConfirmExit(true);
              setConfirmFocus(0);
            }}
            onMouseEnter={() => setSettingsFocus(10)}
          >
            {t("exitLauncher")}
          </button>
          <button
            className={`drawer-item ${settingsFocus === 11 ? "focused" : ""}`}
            onClick={() => setSettingsOpen(false)}
            onMouseEnter={() => setSettingsFocus(11)}
          >
            {t("back")}
          </button>
        </aside>
      )}

      {/* 语言选择弹窗 */}
      {langPickerOpen && (
        <div className="modal-overlay">
          <div className="modal language">
            <h2>{t("selectLanguage")}</h2>
            <div className="lang-list" ref={langListRef}>
              {LANGS.map((l, i) => (
                <button
                  key={l.code}
                  className={`lang-item ${i === langPickerFocus ? "focused" : ""} ${l.code === getLang() ? "current" : ""}`}
                  onClick={() => changeLanguage(l.code)}
                  onMouseEnter={() => setLangPickerFocus(i)}
                >
                  <span className="lang-name">{l.native}</span>
                  {l.code === getLang() && <span className="lang-check">✓</span>}
                </button>
              ))}
            </div>
            <button
              className={`modal-btn ${langPickerFocus === LANGS.length ? "focused" : ""}`}
              onClick={() => setLangPickerOpen(false)}
              onMouseEnter={() => setLangPickerFocus(LANGS.length)}
            >
              {t("back")}
            </button>
          </div>
        </div>
      )}

      {/* 退出确认框 */}
      {confirmExit && (
        <div className="modal-overlay">
          <div className="modal">
            <h2>{t("exitTitle")}</h2>
            <p>{t("exitDesc")}</p>
            <div className="modal-actions">
              <button
                className={`modal-btn primary ${confirmFocus === 0 ? "focused" : ""}`}
                onClick={doExit}
              >
                {t("confirmExit")}
              </button>
              <button
                className={`modal-btn ${confirmFocus === 1 ? "focused" : ""}`}
                onClick={() => setConfirmExit(false)}
              >
                {t("cancel")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 系统操作确认框 */}
      {systemAction && (
        <div className="modal-overlay">
          <div className="modal">
            <h2>{t("systemActionTitle", { action: actionLabel(systemAction) })}</h2>
            <div className="modal-actions">
              <button
                className={`modal-btn primary ${confirmFocus === 0 ? "focused" : ""}`}
                onClick={confirmSystemAction}
                onMouseEnter={() => setConfirmFocus(0)}
              >
                {t("confirm")}
              </button>
              <button
                className={`modal-btn ${confirmFocus === 1 ? "focused" : ""}`}
                onClick={() => setSystemAction(null)}
                onMouseEnter={() => setConfirmFocus(1)}
              >
                {t("cancel")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 按键说明弹窗 */}
      {showKeymap && (
        <div className="modal-overlay">
          <div className="modal keymap">
            <h2>{t("keymapTitle")}</h2>
            <div className="keymap-list">
              {KEYMAP.map((m) => (
                <div key={m.key} className="keymap-row">
                  <span className="keymap-key">{m.key}</span>
                  <span className="keymap-action">{m.action}</span>
                </div>
              ))}
            </div>
            <button className="modal-btn" onClick={() => setShowKeymap(false)}>
              {t("back")}
            </button>
          </div>
        </div>
      )}

      {/* 管理 APP 操作菜单 */}
      {manageTarget && (
        <div className="modal-overlay">
          <div className="modal manage">
            <h2>{t("manageTitle", { name: manageTarget.name })}</h2>
            <div className="manage-actions">
              <button
                className={`modal-btn ${manageAction === 0 ? "focused" : ""}`}
                onClick={() => {
                  setManageAction(0);
                  executeManageAction();
                }}
                onMouseEnter={() => setManageAction(0)}
              >
                {t("delete")}
              </button>
              <button
                className={`modal-btn ${manageAction === 1 ? "focused" : ""}`}
                onClick={() => {
                  setManageAction(1);
                  executeManageAction();
                }}
                onMouseEnter={() => setManageAction(1)}
              >
                {t("rename")}
              </button>
              <button
                className={`modal-btn ${manageAction === 2 ? "focused" : ""}`}
                onClick={() => {
                  setManageAction(2);
                  executeManageAction();
                }}
                onMouseEnter={() => setManageAction(2)}
              >
                {t("moveFront")}
              </button>
              <button
                className={`modal-btn ${manageAction === 3 ? "focused" : ""}`}
                onClick={() => {
                  setManageAction(3);
                  executeManageAction();
                }}
                onMouseEnter={() => setManageAction(3)}
              >
                {t("moveEnd")}
              </button>
              <button
                className={`modal-btn ${manageAction === 4 ? "focused" : ""}`}
                onClick={() => setManageTarget(null)}
                onMouseEnter={() => setManageAction(4)}
              >
                {t("cancel")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 改名弹窗 */}
      {renaming && renameTarget && (
        <div className="modal-overlay">
          <div className="modal">
            <h2>{t("renameTitle", { name: renameTarget.name })}</h2>
            <div className="manual-add">
              <input
                ref={renameInputRef}
                className="manual-input"
                value={renameInput}
                autoFocus
                onChange={(e) => setRenameInput(e.target.value)}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Enter") {
                    confirmRename();
                  } else if (e.key === "Escape") {
                    setRenaming(false);
                  } else if (e.key === "ArrowDown") {
                    setRenameFocus(1);
                    e.currentTarget.blur();
                  }
                }}
              />
              <button
                className={`modal-btn primary ${renameFocus === 1 ? "focused" : ""}`}
                onClick={confirmRename}
                onMouseEnter={() => setRenameFocus(1)}
              >
                {t("confirm")}
              </button>
            </div>
            <button
              className={`modal-btn ${renameFocus === 2 ? "focused" : ""}`}
              onClick={() => setRenaming(false)}
              onMouseEnter={() => setRenameFocus(2)}
            >
              {t("cancel")}
            </button>
          </div>
        </div>
      )}

      {/* 添加 APP 页 */}
      {showAddApp && (
        <div className="modal-overlay">
          <div className="modal add-app">
            <h2>{t("addAppTitle")}</h2>
            <p>{t("addAppHint")}</p>
            <div className="scan-list" ref={scanListRef}>
              {scanList.map((app, i) => (
                <button
                  key={`${app.exe}-${i}`}
                  className={`scan-item ${i === addAppFocus ? "focused" : ""}`}
                  onClick={() => addAppToGrid(app)}
                  onMouseEnter={() => setAddAppFocus(i)}
                  onKeyDown={(e) => e.preventDefault()}
                >
                  {app.icon.startsWith("data:") ? (
                    <img className="scan-icon-img" src={app.icon} alt="" />
                  ) : (
                    <span className="scan-icon">{app.icon || "📦"}</span>
                  )}
                  <span className="scan-name">{app.name}</span>
                </button>
              ))}
              {scanList.length === 0 && <div className="scan-empty">{t("noScanResult")}</div>}
            </div>
            <div className="manual-add">
              <input
                ref={manualInputRef}
                className={`manual-input ${addAppFocus === scanList.length ? "focused" : ""}`}
                placeholder={t("manualPathPlaceholder")}
                value={manualPath}
                onChange={(e) => setManualPath(e.target.value)}
                onFocus={() => setAddAppFocus(scanList.length)}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Enter") {
                    addManualApp();
                  } else if (e.key === "Escape" || e.key === "BrowserBack") {
                    setShowAddApp(false);
                  } else if (e.key === "ArrowDown") {
                    setAddAppFocus(scanList.length + 1);
                    e.currentTarget.blur();
                  } else if (e.key === "ArrowUp") {
                    if (scanList.length > 0) {
                      setAddAppFocus(scanList.length - 1);
                      e.currentTarget.blur();
                    }
                  }
                }}
              />
              <button
                className={`modal-btn primary ${addAppFocus === scanList.length + 1 ? "focused" : ""}`}
                onClick={addManualApp}
                onMouseEnter={() => setAddAppFocus(scanList.length + 1)}
              >
                {t("add")}
              </button>
            </div>
            <button
              className={`modal-btn ${addAppFocus === scanList.length + 2 ? "focused" : ""}`}
              onClick={browseFile}
              onMouseEnter={() => setAddAppFocus(scanList.length + 2)}
            >
              {t("browseFile")}
            </button>
            <button
              className={`modal-btn ${addAppFocus === scanList.length + 3 ? "focused" : ""}`}
              onClick={() => setShowAddApp(false)}
              onMouseEnter={() => setAddAppFocus(scanList.length + 3)}
            >
              {t("back")}
            </button>
          </div>
        </div>
      )}

      {/* 首次启动引导窗 */}
      {firstRun && (
        <div className="modal-overlay">
          <div className="modal first-run">
            <h2>{t("welcomeTitle")}</h2>
            <p>{t("welcomeDesc")}</p>
            <div className="modal-actions">
              <button
                className={`modal-btn ${firstRunFocus === 0 ? "focused" : ""}`}
                onClick={() => chooseMenu("manual")}
              >
                {t("noMenu")}
              </button>
              <button
                className={`modal-btn primary ${firstRunFocus === 1 ? "focused" : ""}`}
                onClick={() => chooseMenu("all")}
              >
                {t("loadAllMenu")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
