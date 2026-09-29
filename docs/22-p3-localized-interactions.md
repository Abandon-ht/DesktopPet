# P3 多语言互动语音与生日

日期：2026-09-29。互动提示音使用用户本机目录 `artifacts/voice/<locale>/<category>/<cue>.wav`，旁边的同名 `.txt` 保存该片段的文本。`locale` 当前为 `zh-CN`、`en-US`、`ja-JP`、`ko-KR`；`category` 为 `greetings`、`care`、`intimacy`。界面顶部切换语言后立即保存到 SQLite；设置面板、菜单栏命令和预录互动语音随之切换。互动语音缺片段时回退到 `zh-CN`。ASR 自动识别和 LLM/TTS 的语言行为仍由各自模型与提示词决定。原始 `artifacts/wav/` 保留给旧配置兼容；新导入脚本不会删除它。

| 文件名（不含扩展名） | 触发条件 | 中文源文件 |
| --- | --- | --- |
| `first_meeting` | 首次启用并显示角色，一次 | 初次见面 |
| `wake` | 点击头部或 KWS 命中后、聆听之前 | 心事 |
| `morning` / `noon` / `evening` / `night` | 本地时区 8／12／19／22 点各一次 | 早上好／午休时间到／太阳落山／快去睡吧 |
| `birthday` | 设置了 `MM-DD` 生日，生日当天角色可见且语音空闲时一次 | 生日 |
| `feed_taste` / `feed_thought` | 成功喂食后交替播放 | 好味道／心意 |
| `greeting` | 主动陪伴进入“招呼”状态时播放 | 去转转 |
| `intimacy_smart` / `intimacy_open` / `intimacy_feeling` / `intimacy_blessing` | 亲密度首次达到 25／50／75／100 | 变聪明啦／思路变开阔了／这种感觉／赐福 |

生日不填写则不触发。`02-29` 只在真实闰年当天播报；年不是存档的一部分。生日在当日问候中优先，其后是首次见面、亲密度档位与定时问候。亲密度档位的最高已播值保存在本机；若一次动作跨过多档，只播放达到的最高档。互动语音只在语音服务启用且空闲时排队，不会打断正在进行的 LLM/TTS 会话；点击头部仍可显式打断。成功喂食的音频可能先于刚达到的亲密度档位播出，后者在待机后的下一次检查播放。

导入脚本 `python3 tools/p3/import-interaction-voices.py` 使用本机 `artifacts/wav/`、`~/Downloads/wiki_audio/纳西妲_asr.txt` 和同目录 `纳西妲/`，需要 `ffmpeg`。脚本把源音频转为 24 kHz 单声道 PCM WAV，按语言与互动类别写入目录，并复制对应文本。已能明确对应的片段为中文 14 条、英文 14 条、日文 10 条、韩文 12 条；其余日／韩片段尚未确信配对，暂回退中文。源文件、转换后的 WAV 和文本均由 `.gitignore` 排除，不随应用或 Git 分发。未来补录只须放到同名的 locale／category 目录，无需改事件代码。旧版自定义平铺 `greeting_dir` 仍能按中文源文件名读取；默认开发配置发现新目录时自动转向新目录。

## KWS 状态和排查

设置中勾选 KWS 会立即保存并启动监听；界面显示关闭、需先启用语音、加载模型、监听中、对话期间暂停、命中或具体错误。先开启 Audio 总开关并设置模型及参考语音，再启用 KWS。若缺模型，检查 `models/local/sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20/` 内的 `encoder/decoder/joiner` chunk-16 ONNX 与 `tokens.txt`；若显示麦克风错误，检查系统权限和默认输入设备。KWS 只有待机时监听，因此测试须等当前播放及会话结束。[官方模型说明](https://k2-fsa.github.io/sherpa/onnx/kws/pretrained_models/index.html)使用 chunk-16 模型，默认阈值 0.25，词表必须是 `phone+ppinyin` 转换的 token 序列。中文合成语音的本机模型测试已命中；真人麦克风与英文“Hello Nahida”的命中率仍要人工检查。此次移除设置面板中的 sherpa-mlx ASR 选项；旧字段仅为存档兼容保留。

## 本机验收

1. 关闭旧版，再启动新测试包。检查顶部中／英／日／韩界面切换，重新打开面板后语言仍在；各语言若有 `wake.wav`，点击头部试听对应语言。
2. 在 Audio 填生日 `MM-DD`，设成今天并保存，角色显示、语音待机时应播 `birthday`，当天重启不重复。改为无效日期时保存应提示错误。
3. 在养成区连续成功喂食，听到 `feed_taste`、`feed_thought` 轮换；亲密度达到 25、50、75、100 后各播一次。主动陪伴开启并进入“招呼”时播放 `greeting`。
4. 在 KWS 勾选开关，确认显示“监听中”后说“你好纳西妲”；出现“命中”时应先播 `wake` 再聆听。错误应直接显示在 KWS 卡片中。此条需用户本人对麦克风和本地声学环境验收。
