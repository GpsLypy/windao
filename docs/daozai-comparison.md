# Windows 版与岛仔的功能对照

对照基线为岛仔 v0.6.70（0615f19）与 NotchCalendar Windows 0.1.4 开发源码。0.1.3（e69fc16）之后的新增项按源码记录，发布情况以 Releases 为准。第三方账号、播放器以及外设的真实兼容性需要在 Windows 上分别验证。[岛仔版本说明][release] · [版本 README][readme] · [使用指南][usage]

| 功能 | 岛仔 v0.6.70 | Windows 入口及当前实现 | 仍有差异 |
| --- | --- | --- | --- |
| 停靠、缩放和避让 | 四边停靠、跨屏、80%–140% 缩放、OLED 偏移、遮挡避让。README 图例列三边，底部以标签源码为准。[停靠][dock] | 设置 → 胶囊与停靠，选屏、调位、极简、自动避让、全屏隐藏、Ctrl 拖动。[停靠](../src/dock.rs) · [窗口](../src/windows.rs) | 多显示器混合 DPI、自动隐藏任务栏的行为仍需逐机验证。 |
| 外观和伙伴 | 刘海及胶囊、五种皮肤、动态伙伴、封面取色。[外观][render] | 两种轮廓、五套皮肤、封面取色、透明度、猫伙伴及其状态动画。设置中可拖动排列横向胶囊的时钟、天气、AI、系统指标、伙伴和消息角标。工作台可切换主题、排列页面，胶囊可打开功能目录。[皮肤](../src/ui/capsule/skin.rs) · [伙伴](../src/ui/capsule/companion.rs) | 毛玻璃合成、封面色和连续动效需在 Windows 显卡及系统主题上验收。 |
| 截图与录屏 | 可从兼容工具的捕获结果隐藏浮窗。[捕获][capture] | 设置内开启胶囊捕获排除，Win32 窗口应用该设置。[窗口](../src/windows.rs) | 不同录屏软件对系统排除标记的支持需分别验证。 |
| 微信、QQ 和通知 | 任务栏新消息、分账号跳转、QQ 系统通知、预览和托盘唤起。[消息][messenger] | 通知页记录五种聊天工具的任务栏闪烁，按来源窗口分组，胶囊红点可打开通知页。可激活已验证窗口或请求打开微信客户端。具有包身份的版本支持用户授权后读取 QQ 系统通知、另行开启脱敏预览。[任务栏](../src/windows/taskbar_notifications.rs) · [QQ](../src/windows/qq_notifications.rs) | 窗口号仅区分本次运行的窗口。微信客户端决定通用唤起打开哪个账号。QQ AUMID、权限弹窗、真实通知及多账号托盘均待实机验证。 |
| 邮箱和邮件邀请 | 多个 IMAP 邮箱、邮件阅读与邀请、验证码卡片。[邮件][mail] | 本次源码实现多账户 TLS IMAP、最近 30 封、整页标已读、本机或可选服务器废纸篓及还原、独立正文窗口、邀请预览确认导入及保存。支持 HTML 转为离线文本、打开网页链接及点击复制已显示的验证码，30/60/300 秒后重新遮盖。[邮箱](../src/ui/mailbox.rs) | IMAP 协议及本机标记已有模拟测试，真实供应商登录、Windows 凭据和界面待验。 |
| 播放与歌词 | 系统媒体会话、音量、播放进度、播放器专用适配、曲目 ID 找词。[媒体][smtc] · [适配][musiccompat] | 工作台及胶囊控制媒体会话，播放页有应用会话音量、静音、LRC 和 LRCLIB 搜词、网易云歌曲 ID 查词、逐行跟随及顶部歌词。[音乐](../src/ui/music.rs) · [音量](../src/ui/music/mixer.rs) | 依赖对应播放器公开的媒体和音频会话信息；各客户端、歌名歧义、歌曲 ID 与真机音量控制待验。 |
| 日历与会议 | 月历、时间线、农历、ICS、邮件邀请、CalDAV、Exchange 和多个服务商设置。[日历][calendar] | 本地事件、农历节气、提醒、会议笔记、ICS 导入导出与订阅、CalDAV、Exchange ActiveSync 读取；提供飞书、钉钉、企业微信及 iCloud 预设。邮件邀请先预览再确认，同 UID 与重复实例在列表和提醒中合并，各订阅独立缓存。支持开始时或提前 5、10、15、30、60 分钟提醒。[同步](../src/calendar_sync.rs) · [Exchange](../src/calendar_sync/exchange.rs) · [日历](../src/ui/calendar.rs) | Exchange、CalDAV 的真实服务端、账号授权与重复日程兼容性待验。 |
| 待办、专注和健康 | 每日、每周待办，专注与休息、饮水目标、时段和撤销。[使用指南][usage] | 待办列表、四象限、日历、统计、桌面待办、重复规则和导入。专注页可设每日饮水目标及每杯毫升、记录与撤销、跨午夜提醒时段、午休暂停及久坐提醒。[待办](../src/workspace/todo.rs) · [健康](../src/workspace/health.rs) | 旧版按次数保存的记录仅显示次数。桌面待办及睡眠恢复后的系统通知需在 Windows 验收。 |
| 文件和剪贴板 | 文件可拖入拖出，剪贴板 30 条、固定、重启保留。[置物][filedrag] · [剪贴板][clipboard] | 文件暂存支持添加、定位、打开及拖出。剪贴板可主动开启采集、固定、重新复制及清空，最多 30 条，使用当前 Windows 账户加密保存，列表遮盖识别到的验证码。[拖出](../src/ui/native_tools/drag.rs) · [剪贴板](../src/ui/clipboard.rs) | 跨应用拖放、受保护剪贴板格式及重启恢复需在 Windows 验收。 |
| 笔记与资料 | 便签与插件式日常工具。[插件][plugins] | 便笺、卡片、会议笔记、我的日子、本机数据备份恢复。设置 → 数据与备份可在满足空工作区条件时导入旧版资料。[笔记](../src/ui/notes_extra.rs) · [备份](../src/backup.rs) | 两款产品的数据结构及全部迁移范围不同。 |
| 天气 | Open-Meteo、自动定位、15 分钟刷新、缓存、和风天气及可选常驻温度。[天气][weather] · [和风][qweather] | 本次源码支持两个天气源、湿度与风速、日出日落、七日预报、按来源与城市隔离的离线缓存、可关闭自动刷新（Open-Meteo 每 15 分钟、和风实况每小时）、手动刷新冷却 30 秒、普通失败两分钟后重试、双服务主动网络定位、回车搜索与确认城市、五个收藏城市、可选胶囊天气图标和温度、和风天气本机月请求统计与上限，以及 6 小时七日预报缓存。[天气页](../src/ui/weather.rs) | 和风天气真实凭据、Windows 网络定位与常驻温度界面待验。 |
| 性能与无线设备 | 性能、设备电量、常驻指标，电源、网络、音量和输入事件提示。[性能][perf] · [蓝牙][wireless] | 性能页显示 CPU、内存和网速，可选显示电源、电量、大小写锁定、输入语言及横向胶囊常驻指标。可选短提示涵盖网络、系统主音量／静音和蓝牙变化；无线页读取配对设备和兼容 BLE 标准电量。[性能](../src/ui/system_metrics.rs) · [状态变化](../src/windows/status_changes.rs) · [蓝牙](../src/ui/wireless.rs) | 蓝牙设备覆盖、音频切换和 Windows 输入法待实机验证。厂商私有电量协议尚未覆盖。 |
| 插件与内置工具 | 插件目录、本地安装和升级、内置兼容包。[市场][market] · [安装][installed] | 目录、随包模板及本地 JSON 安装；可检查目录版并手动更新，保留启用状态及匹配的勾选项。工作台另有世界时钟、金价、工时收入与纪念日。[插件](../src/ui/plugins.rs) · [组件格式](widget-manifest.md) | 内置 JSON 组件与岛仔原生代码、皮肤和硬件扩展包有格式差异。 |
| AI 额度 | Codex、Claude、Kimi、Grok Bot、Gemini、GLM、WorkBuddy、DeepSeek、Kiro、中转站，标签更新另外列出 Antigravity CLI。[版本说明][release] | AI 用量页读取 Codex、Claude 本地用量。可绑定独立 Codex 目录、Claude 桌面版组织用量历史、Claude Code 与 Antigravity CLI 官方状态栏。Antigravity 按额度分组显示剩余比例或数量、重置时间及当前模型。[Google 状态栏格式][agy]。账户页支持 DeepSeek／Kimi API 余额、GLM Coding Plan、Kimi Code、Kiro、WorkBuddy、中转站，以及 Gemini、Grok、Claude 的报告来源。[本地用量](../src/ai_usage.rs) · [账号](../src/ui/provider_balances.rs) | 中转站支持直接导入 CC Switch 通用、New API、Sub2API 模板，以及同源 GET 与数值换算的自定义查询。Gemini／Grok 的官网可见百分比可由分享扩展写入手选的账号文件。能确认页面账号时，可开启每 5 分钟刷新；离开页面、切换账号或失去写权限后停用。未登录探测仅验证鉴权响应，实际账户额度、Grok 登录页面及已登录 Antigravity 实机回调待验。 |
| 安装与更新 | Windows 10/11 x64 安装程序与 ZIP、应用内下载校验更新、诊断包、D3D12 回退。[更新][updates] · [诊断][diagnostics] | 提供 Windows 10 2004+ / 11 x64 的 NSIS 与 ZIP 构建。可通过 Windows CI 获取专供 Windows 11 测试的未签名 MSIX，另支持自行签名构建、应用内校验下载更新、诊断 ZIP、OpenGL 初始化失败时回退 D3D12。[打包](../package.py) · [MSIX](../build-msix.ps1) · [更新](../src/updates/desktop.rs) · [渲染](../src/app.rs) | 安装器升级与进程交接、GPU 回退、MSIX 签名安装及真实授权待 Windows 验证。Windows 10 的系统兼容性待验。 |

[release]: https://github.com/Renly1994/xiaotang-daozai/releases/tag/v0.6.70
[readme]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/README.md
[usage]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/docs/USAGE.md
[dock]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/model.rs#L105
[render]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/renderer.rs
[capture]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/native_capture.rs
[smtc]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/smtc.rs
[musiccompat]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/music_compat.rs
[calendar]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/calendar.rs#L30
[messenger]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/messenger.rs#L18
[mail]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/mail.rs
[filedrag]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/system.rs#L850
[clipboard]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/clipboard.rs
[weather]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/weather.rs
[qweather]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/qweather.rs
[perf]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/performance_ui.rs
[wireless]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/bluetooth_ui.rs
[market]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/spike/market.rs
[installed]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/spike/local_ui.rs#L1108
[plugins]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/docs/plugins/local-install.md
[updates]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/updates.rs
[diagnostics]: https://github.com/Renly1994/xiaotang-daozai/blob/0615f19786d3d5b6b4c2c74be987fbe855e9fe03/src/diagnostics_tool.rs

[agy]: https://antigravity.google/docs/cli/statusline/
