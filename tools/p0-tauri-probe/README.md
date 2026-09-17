# P0-04 Tauri 与真实渲染宿主

这是本机开发试件：Tauri 2.11.5（tauri-build 2.6.3）拥有控制窗口和应用事件循环，独立 `p0-probe host` 进程拥有 winit/Mocari 角色窗口。不是签名发行包，不含角色资源。图标是项目生成的纯色圆形。

从工程根目录启动，替换为本地模型绝对路径：

```sh
cargo build --release --workspace --locked
P0_MODEL="/absolute/path/model.model3.json" target/release/p0-tauri-probe
```

控制窗口标题显示连接状态。退出请求先通知后台 Supervisor，完成角色 shutdown/退出或超时清理后再退出 Tauri。可设置 `P0_AUTO_EXIT_SECONDS=8` 自动发起 Tauri 退出请求，走相同的 ExitRequested 处理路径。此试件关闭控制窗口即退出；未来正式产品的“关闭设置仍保留托盘”尚未实现。

宿主启动参数为 `host MODEL`，启用动态输入，取消 standalone window 的定时退出。hello/ping/shutdown 经 winit 主线程确认后才回复，shutdown 回复 flush 后才请求事件循环退出，stdin EOF 或协议错误也会投递关闭事件。日志使用 stderr，协议使用 stdout。版本校验、帧限制等复用管道试件 server。

Supervisor 在独立线程运行：单请求在途、5 秒响应期限、每秒心跳、退出信号可打断心跳等待和退避；60 秒内第 3 次失败后停止重启。重连会新建角色窗口，不恢复原窗口位置、表情或后续业务状态。

## 自动验证

保持桌面解锁，关闭其他同类试件后运行：

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 tools/p0-ipc-probe/verify.py
python3 tools/p0-tauri-probe/verify.py "/absolute/path/model.model3.json"
```

验证会打开临时角色/控制窗口，并只对本次启动的进程注入终止故障。覆盖直接宿主正常协议、EOF、错版本；Tauri 正常退出、宿主崩溃恢复、父进程死亡清理和三次失败耗尽重启预算。日志位于忽略目录 `artifacts/local/p0/bridge/`。

自动结果以 IPC 响应、退出码和 PID 清理为依据，不是所有 UI 操作的视觉验收。点击关闭按钮/Cmd-Q、长时间持续拖拽是否影响心跳、锁屏/睡眠、多屏、正式 sidecar 签名与进程树句柄继承仍需另测。当前宿主没有独立的父进程心跳看门狗，父进程暂停但仍持有管道时，不等同于父进程死亡测试。

参考：[Tauri App::run](https://docs.rs/tauri/2.11.5/tauri/struct.App.html)、[Tauri 配置](https://v2.tauri.app/reference/config/)。
