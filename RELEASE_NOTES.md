更新说明：
1. 版本升级至 v1.0.36，同步更新前后端与 vision 配置。
2. 新增 `upstream_transport` 上游多协议传输引擎，支持 HTTP/3 (QUIC) 与 HTTP/2 竞速、QUIC 保活、断链重试回退及 HTTP/2 Extended CONNECT WebSocket 通道。
3. 新增 `bridge` 协议桥接模块，实现本地 HTTP / SSE / WebSocket 与上游 WebSocket / SSE / 非流式 HTTPS 的双向桥接及自动降级重试，并在修改上游或代理配置时自动清理复用连接。
4. 完善代理网关连接状态观测与协议桥接单元测试。
