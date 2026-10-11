# SocksNode 静态代码审查记录（2026-10-11）

## 范围

- Rust/Tauri：节点 CRUD、生命周期、进程回收、故障切换、IP 选择、配置持久化、日志、网卡绑定、托盘、IPC。
- React/TypeScript：表单、节点状态、日志异步加载、事件订阅、敏感输入、设置操作。
- AnyTLS-Go fork：客户端启动参数、TCP/UDP、SOCKS5 UDP 控制、网卡绑定；全模块测试、vet 与 race 检查。
- 资源和脚本：内置 anytls-client 重建、Tauri 配置和权限、DMG 脚本语法及源码一致性。
- 没有建立真实节点连接、执行 UDP 端到端请求、测试 GUI、DMG 签名或安装。

## 修复清单

1. 启动、停止、更新、删除、批量操作和故障切换的并发竞态，统一代际取消检查与节点锁。
2. 异步 DNS/测速向已停止或已更新的节点写回过期结果。
3. 批量任务与 IP 探测没有足够的并发上限；增加批次与探测工作线程上限。
4. 同端口并发创建和配置写入失败时内存、磁盘状态不一致；保存成功后才提交变更。
5. 无效、重复节点 UUID 被用作目录路径的风险；导入配置时拒绝。
6. 停止与退出时遗漏子进程，以及启动时使用范围过广的 pkill；收紧进程归属验证并最终回收。
7. 编辑丢失 enabled、UDP 配置只写入未下发内核、协议类型可填未实现的协议；统一界面与内核行为。
8. 测试 iLatency Cookie 时误覆盖原凭证、curl 重定向和命令行泄露 Cookie；已加固。
9. React StrictMode、窗口事件订阅及日志异步切换的清理竞态。
10. 磁盘日志无限增长、文件权限与日志消息无上限；现在轮转并限制日志长度。
11. 物理网卡检测缓存永久不刷新；改为短期缓存。
12. macOS 托盘批量操作同步阻塞事件线程；移到工作线程。
13. 发布脚本路径包含单引号以及失败时提前删除旧 DMG 的风险。
14. Cookie 预览 UTF-8 字节索引可能 panic。

## 验证命令

    ./node_modules/.bin/tsc --noEmit -p tsconfig.json
    npm run build:web
    cd src-tauri && cargo clippy --locked --offline --all-targets -- -D warnings
    cd src-tauri && cargo test --locked --offline
    cd third_party/anytls-go && go test -race ./... && go vet ./...
    bash -n scripts/sign-dmg.sh
    git diff --check

内置二进制以仓库内 third_party/anytls-go/cmd/client 为源构建到 src-tauri/resources/anytls-client。

## 交给端到端验证的事项

- 真实 AnyTLS 服务端 TCP、TLS 证书、SOCKS5 UDP ASSOCIATE/UoT 和禁用 UDP 的行为。
- iLatency Cookie 与多地 DNS 在真实 API 环境中的响应及错误处理。
- Surge/Clash TUN、网络切换、双栈/IPv6、物理网卡绑定的真实选路；Go 内核按启动时选定的出口工作，切换网络后可能需重启节点。
- 慢网络下的批量测速、长连接、取消延迟和自动故障切换。
- 托盘、登录项、异常退出、Gatekeeper、DMG 签名、安装等系统集成。
- 第三方依赖安全公告、外部 API 变化以及未覆盖的协议边界；Go fork 底层协议包缺少单元测试。
- 跳过上游证书验证目前默认开启。这是有兼容性影响的安全取舍，是否改为默认关闭需要另行决定。

本记录表明已完成的静态检查和已知边界，不构成绝对无 BUG 的保证。
