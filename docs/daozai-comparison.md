# WinDao 与岛仔 v0.6.70 的功能与源码对照

以 2026 年 11 月 25 日发布的岛仔 v0.6.70（`0615f19`）和 Windows Rust 原生预览 `0.10.0-alpha.3` 的工作区源码为基准。本文所称「Windows 预览」仅指 `native/` 中的实现。表内功能依据公开文档和静态源码核对，真机交互、第三方账号和硬件覆盖仍待实测。 [岛仔发布说明][release] · [标签 README][readme] · [标签更新记录][changelog]

## WinDao 0.1.0

- **UI 与停靠**：增加底部停靠、Ctrl 拖动跨屏、Windows 捕获排除、半透明外观和工作台页面快捷键。WinDao 使用自己的胶囊布局、颜色和操作入口。参见 [停靠计算](../src/dock.rs)、[设置](../src/ui/dock_settings.rs)与 [Win32 窗口](../src/windows.rs)。
- **音乐**：两边都使用 Windows 系统媒体会话。WinDao 在会话及播放事件触发时更新快照，基于媒体时间轴插值歌词位置。新版本展开后提供逐行歌词、高亮和可切换的滚动跟随。参见 [媒体会话](../src/music/windows_media.rs)、[音乐页](../src/ui/music.rs)。
- **日历**：WinDao 从 HTTPS ICS 或 CalDAV 拉取内容。CalDAV 使用 WebDAV 发现日历，经 HTTPS 刷新只读缓存，凭据由 Windows DPAPI 保存。新增按天浏览、时分选择和离线农历节气。农历换算移植自岛仔的 MIT 模块并随包保留许可。参见 [CalDAV](../src/calendar_sync/caldav.rs)、[日历页](../src/ui/calendar.rs)、[农历](../src/ui/lunar_calendar.rs)。
- **通知**：岛仔分别处理任务栏提醒和已授权的 QQ 通知。WinDao 经 Windows 托盘发出自身提醒，并记录已投递历史。通知页也可选监听四款聊天工具的任务栏闪烁，仅记录来源和时间。参见 [提醒调度](../src/workspace/reminders.rs)、[任务栏监听](../src/windows/taskbar_notifications.rs)和[存储](../src/workspace.rs)。
- **系统工具**：原生性能页以 Windows 系统计数器获取 CPU、内存、网速。世界时钟按 IANA 时区规则换算城市时间。参见 [计数器](../src/system_metrics.rs)与[性能页](../src/ui/system_metrics.rs)。

- **桌面工具**：WinDao 把「我的日子」、现货金价和工时收入估算做成内置功能。便笺与世界时钟也可直接使用。剪贴板历史由用户开启并保留在当前运行会话。参见 [我的日子](../src/ui/dates.rs)、[金价](../src/ui/gold.rs)、[实时工资](../src/ui/salary.rs)和[剪贴板](../src/ui/clipboard.rs)。
- **天气与设备**：天气按城市向 Open-Meteo 查询实况与七日预报。无线页从 Windows 读取已配对的设备与连接状态，为兼容低功耗蓝牙设备提供标准 GATT 电量读取。参见 [天气](../src/ui/weather.rs)和[无线设备](../src/ui/wireless.rs)。
- **插件集市**：WinDao 支持从在线目录选装声明式组件，也可安装随包模板和导入本地 JSON。已安装组件可离线管理，数据保存在本机。参见 [组件格式](widget-manifest.md)、[在线组件目录](../catalog/widgets.json)和[插件页](../src/ui/plugins.rs)。

插件自动升级、厂商专用设备电量、邮箱、QQ 系统通知、聊天正文与账号跳转、AI 额度仍有差距。下表保留重构前的功能基线。

## 重构前的 Windows 基线

| 模块 | 岛仔 v0.6.70 | Windows 原生预览 |
| --- | --- | --- |
| 停靠与避让 | 顶部、底部、左右四边，底部向上展开并跟随自动隐藏任务栏，支持跨屏拖动、显示器选择、缩放、沿边调位、OLED 偏移，支持浏览器、自选应用与全屏避让。标签 README 的停靠说明仍列出顶部和左右侧，发布说明与代码已覆盖底部。 [位置设置][dock] · [拖动逻辑][drag] | 顶部和左右三边，80%–140% 缩放、沿边调位、固定显示器或跟随主屏、OLED 偏移、浏览器和自选应用遮盖避让、前台全屏隐藏。底部停靠、跨屏拖动和任务栏跟随仍有差距。 |
| 外观与交互 | 刘海、胶囊两种形态，经典黑、奶油、深浅毛玻璃、悬浮云等皮肤，支持桌面伙伴和常驻组件。发布说明记录固定顶栏、翻页动画和透明点击边界优化。兼容渲染的流畅度数字来自项目发布材料中的特定设备条件。 [皮肤与停靠设置][dock] · [渲染降级][render] | 托盘、全局热键、鼠标悬停或点击展开，支持极简、功能页开关与排序、12／24 小时制、明暗／跟随系统和减少动态效果。原生预览使用 OpenGL 渲染路径。皮肤组合、材质、宠物和 Direct3D 12 兼容渲染仍有差距。 |
| 截图录屏 | 支持在兼容工具的捕获画面中隐藏岛体，标签实现调用 Windows 窗口捕获排除接口。 [捕获排除][capture] | 截图与录制仍待迁移。胶囊自身的捕获排除仍有差距。 |
| 音乐与歌词 | 通过 Windows 媒体会话读取和控制播放，提供音量控制、封面补全及多来源歌词。补充网易云、QQ 音乐、酷狗、汽水音乐和波点的适配。此版本加入系统事件唤醒，优化部分播放器进度读取及波点曲目信息。 [媒体会话][smtc] · [歌词][lyrics] · [播放器兼容][musiccompat] · [波点][bodian] | 已有系统媒体会话事件监听、曲目与封面、曲目进度、播放／暂停、切歌和拖动进度，具体能力取决于会话上报。支持本地 LRC 导入、从 LRCLIB 手动查询，以及顶部收起显示当前歌词。播放器专用适配、应用音量、封面补全与自动歌词源选择仍有差距。 |
| 日历和外部来源 | 本机日程、ICS 导入、月历及按日期日程、农历／节气、日期常驻和日期时间选择器。日历聚合兼容的 CalDAV、iCloud、飞书、钉钉、企业微信及 Exchange ActiveSync，并合并邮件邀请。是否能同步取决于服务商实际授权和配置。 [日期日程][calendarui] · [日历来源][calendar] · [农历][lunar] | 月历、日期选择与当日安排、本机编辑、ICS 文件导入预览及导出、只读 HTTPS ICS 订阅与通用 CalDAV 发现和同步。刷新失败保留缓存。同一服务器发现依赖服务器配置。农历、节气、日期常驻、独立服务商向导、Exchange ActiveSync 和邮件邀请接入仍有差距。 |
| 待办与专注 | 待办支持单次、每周选日和每天重复，专注／休息计时可在收起状态展示。饮水和久坐可独立设提醒，并可记录饮水。 [使用指南][usage] · [提醒调度][app] | 待办支持列表、四象限、日历和统计，支持每周选日、每月、每年重复，并按日期记录完成。提供岛仔 `todo.txt` 导入。专注和休息按绝对截止时间计时，有专注记录及完成提醒。饮水和久坐管理仍有差距。 |
| 系统与聊天通知 | 微信、企业微信、飞书和钉钉通过任务栏提醒机制识别，QQ 接入授权的系统通知，支持按账号定位窗口。可配置通知预览、遮罩验证码，发布说明包含钉钉和飞书通知点击及清除修复。 [客户端适配][messenger] · [通知入口][system] | 日程、待办、专注完成可经 Windows 托盘发出本应用的提醒。读取其他应用的未读提示、消息预览、按账号唤起和验证码通知仍有差距。 |
| 邮件与验证码 | 多邮箱 IMAP、正文阅读、验证码识别和复制，可仅提醒验证码，支持邮件管理、正式邀请进入日程以及 Gmail 使用 Windows 手动代理。 [邮件][mail] · [邮件管理][mailmanage] · [邮件代理][mailproxy] | 当前范围为本地日程与提醒，邮件接入仍有差距。 |
| 文件与剪贴板 | 文件拖入暂存、打开／定位与拖出，文字剪贴板保留最近记录并可固定。发布说明包含置物架和剪贴板保持展开。 [文件拖出][filedrag] · [剪贴板][clipboard] | 文件暂存支持拖入、选择多项、打开、定位和复制路径。旧版暂存记录迁移、拖出、收藏目录及文字剪贴板历史仍有差距。 |
| 便笺、卡片与迁移 | 便签本列入首批插件，宿主有独立插件数据存储。 [首批插件发布说明][release] · [宿主插件文档][plugins] | 内建便笺、卡片和会议笔记。可复制同机旧版核心工作区并导出和恢复工作区备份。原 Windows 授权沿用原安装标识和激活码。 |
| 天气与 AI 额度 | Open-Meteo 自动定位／选城、离线缓存、常驻天气与可选和风天气。AI 额度页接入多个服务，发布说明新增 Antigravity CLI。 [标签 README][readme] · [天气][weather] · [和风天气][qweather] · [Antigravity][antigravity] | 天气、AI 额度和 AI 功能仍待迁移。 |
| 状态、性能与无线设备 | 电源、音量、网络变化、输入状态等提示。新增系统性能与无线设备页，可选择常驻指标。设备电量接入依赖系统报告和设备兼容包。 [系统状态][system] · [性能页][perf] · [蓝牙入口][wireless] · [插件文档][plugins] | 支持胶囊时钟、专注和媒体状态。系统性能详情、网速、设备电量和通用设备提示仍有差距。 |
| 插件集市 | 集市和已安装管理、浏览排序、本地导入、启用及升级后的数据保留有标签源码。首批八款插件的正式分发记录来自发布说明，部分插件在标签中有开发样例。 [市场实现][market] · [安装管理][installed] · [首批插件发布说明][release] | 当前功能由应用内建，插件宿主、插件集市与兼容包分发仍有差距。 |
| 安装、更新与诊断 | 发布 Windows 10／11 x64 安装程序、ZIP 和 SHA-256。支持应用内在线升级及诊断 ZIP。官方文档报告 Windows 11 有本机实测，Windows 10 和更多显卡／多屏组合仍待覆盖。 [标签 README][readme] · [发布资产][release] · [更新][updates] · [诊断][diagnostics] | 原生预览面向 Windows 11 x64，仓库含 NSIS 安装定义与 ZIP 打包脚本。预览包为未签名构建，自动更新、诊断 ZIP 及不同 Windows 配置的系统验收仍有差距。 |

岛仔发布说明列出的首批插件为液态玻璃、我的日子、现货金价、实时工资、便签本、世界时钟、页面快捷切换和无线设备兼容包。集市客户端与部分样例可在标签中追溯。首批八款的在线目录、下载与安装结果仍需单独验收。 [发布说明][release] · [插件文档][plugins]

[release]: https://github.com/Renly1994/xiaotang-daozai/releases/tag/v0.6.70
[readme]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/README.md
[changelog]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/CHANGELOG.md#v0670
[usage]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/docs/USAGE.md
[dock]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/model.rs#L105
[drag]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/app.rs#L3354
[app]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/app.rs#L4278
[render]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/renderer.rs
[capture]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/native_capture.rs
[smtc]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/smtc.rs
[lyrics]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/lyrics.rs#L204
[musiccompat]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/music_compat.rs
[bodian]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/bodian.rs
[calendarui]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/calendar_ui.rs#L323
[calendar]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/calendar.rs#L30
[lunar]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/lunar_calendar.rs
[messenger]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/messenger.rs#L18
[system]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/system.rs
[mail]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/mail.rs
[mailmanage]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/mail_manage.rs
[mailproxy]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/mail_proxy.rs
[filedrag]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/system.rs#L850
[clipboard]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/clipboard.rs
[weather]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/weather.rs
[qweather]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/qweather.rs
[antigravity]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/antigravity_quota/mod.rs
[perf]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/performance_ui.rs
[wireless]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/bluetooth_ui.rs
[market]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/spike/market.rs
[installed]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/spike/local_ui.rs#L1108
[plugins]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/docs/plugins/local-install.md
[updates]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/updates.rs
[diagnostics]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/diagnostics_tool.rs
