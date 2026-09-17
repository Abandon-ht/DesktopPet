# P0-04 管道协议试件

这是不含窗口与模型的独立子进程，用于隔离协议与退出行为。共享 server/Supervisor 已接入 [Tauri 与真实渲染宿主试件](../p0-tauri-probe/README.md)，本目录保留无图形故障测试入口。

从工程根目录运行：

```sh
cargo build --release --locked -p p0-ipc-probe
python3 tools/p0-ipc-probe/verify.py
cargo test --locked -p p0-ipc-probe
target/release/p0-supervisor target/release/p0-ipc-probe
```

stdin/stdout 为逐行 UTF-8 JSON，stderr 为诊断。每帧最大 256 KiB，包含末尾换行；解析前限制输入长度。必需字段为 `protocol_version=1`、非空 `session_id`、从 1 连续递增的 `sequence`、非空 `request_id`、`type` 和对象 `payload`。会话内 session_id 不变，响应回显 sequence/request_id；目前无业务副作用，不实现 request_id 去重缓存。

首条 `hello` 返回 `ready`，能力列表只声明 `ping` 和 `shutdown`；之后分别返回 `pong` 和 `stopped`。错误输入向 stderr 输出明确原因并以非零码退出，不能把 EOF 误当作 ready。正常 shutdown 或帧边界 EOF 退出成功；半帧 EOF 视为协议错误。stdout 不输出日志。

验证脚本使用真实 subprocess 和 5 秒超时，覆盖 12 个正常/错误协议与 EOF 场景，另外验证强制结束父进程后，继承管道关闭能使子试件退出。此行为依赖没有其他进程持有 stdin 写端；接入 Tauri 后仍须重测句柄继承与实际窗口清理。

## Supervisor

Rust 库 `supervisor::Host` 管理一个自建子进程。请求/响应通道各容量 1，至多一个在途请求；阻塞管道读写放在专用线程。握手、ping、shutdown 响应验证版本、会话、序号、请求 ID 和消息类型。启动还检查 ping/shutdown 能力与消息上限。响应超时或校验失败会 kill/wait 回收子进程，废弃旧会话；shutdown 收到 stopped 后仍等待真正退出，超过期限执行兜底清理。Drop 也会清理未关闭的子进程。

`p0-supervisor HOST_EXECUTABLE` 是有限时长的监督试跑：2 秒响应期限，每秒 ping，完成 3 次健康检查后正常退出。失败后新建进程和会话，首次退避 250 ms，第二次 500 ms；60 秒滚动窗口内累计第 3 次失败后停止重启并非零退出。期间短暂成功不清空失败计数。库可由后续 Tauri 后台工作线程复用，不能直接在 GUI 事件循环调用阻塞 API。

9 项 Supervisor 测试覆盖真实宿主握手/心跳/退出、握手超时、心跳超时与进程回收、错误响应标识和超长帧、确认退出但进程挂起、Drop 清理、崩溃后恢复、重启预算耗尽和窗口过期。故障注入使用本地 Python 子进程，不读取角色或操控桌面。

边界：目前只监督直接子进程；不允许宿主把控制管道继承给其后代，否则线程清理可能等待未关闭的管道。Tauri 试件已增加可取消的持续监控，并将消息投递到真实 winit 主线程；这里的子试件本身仍依赖 EOF 退出，没有独立心跳看门狗，也没有进程树管理。集成结果与剩余限制见 Tauri 试件说明。
