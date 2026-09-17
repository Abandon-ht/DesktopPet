# P0-04 管道协议试件

这是不含窗口与模型的独立子进程，用于隔离协议与退出行为。尚未接入 Tauri 或真实渲染宿主，不代表 P0-04 整体验收通过。

从工程根目录运行：

```sh
cargo build --release --locked --bin p0-ipc-probe
python3 tools/p0-ipc-probe/verify.py
```

stdin/stdout 为逐行 UTF-8 JSON，stderr 为诊断。每帧最大 256 KiB，包含末尾换行；解析前限制输入长度。必需字段为 `protocol_version=1`、非空 `session_id`、从 1 连续递增的 `sequence`、非空 `request_id`、`type` 和对象 `payload`。会话内 session_id 不变，响应回显 sequence/request_id；目前无业务副作用，不实现 request_id 去重缓存。

首条 `hello` 返回 `ready`，能力列表只声明 `ping` 和 `shutdown`；之后分别返回 `pong` 和 `stopped`。错误输入向 stderr 输出明确原因并以非零码退出，不能把 EOF 误当作 ready。正常 shutdown 或帧边界 EOF 退出成功；半帧 EOF 视为协议错误。stdout 不输出日志。

验证脚本使用真实 subprocess 和 5 秒超时，覆盖 12 个正常/错误协议与 EOF 场景，另外验证强制结束父进程后，继承管道关闭能使子试件退出。此行为依赖没有其他进程持有 stdin 写端；接入 Tauri 后仍须重测句柄继承与实际窗口清理。

下一步：实现可供 Tauri 调用的 Supervisor，增加握手/心跳超时、有界响应通道、60 秒最多 3 次失败的重启预算；随后接入实际 winit 宿主，通过主线程事件投递处理关闭。当前试件不具备超时监控、崩溃自动恢复、业务命令或角色资源加载能力。
