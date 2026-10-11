# SocksNode (Tauri)

把上游代理节点（如 anytls）转发为本地 SOCKS5 的轻量桌面工具。

- 内置 `anytls-client` 内核（约 6MB，替代原 41MB mihomo）
- 已打补丁支持 SOCKS5 UDP（UoT 握手使用真实目标地址），并扩展了 `-udp` / `-bind` / `-iface` / `-insecure`，密码通过 `SOCKSNODE_PASSWORD` 环境变量传入（版本 `anytls/0.0.13-socksnode5`）
- 每个节点独立进程；禁用 UDP 的节点会向内核传入 `-udp=false`
- 仅本地 SOCKS，不启用系统代理 / TUN
- Quiet Console 风格界面
- **Tauri 2** 壳（系统 WebView 壳）

> UDP 补丁说明见 `third_party/anytls-go-patches/`

## 节点

仓库**不内置任何节点或凭据**。首次启动时列表为空，请在界面中手动添加自己的
上游节点。节点密码与 iLatency Cookie 会写入应用数据目录（macOS 上以 0600
权限保存），但仍建议不要在共享账号下使用。

## 开发

需要：Node 20+、Rust stable、Xcode CLT（macOS）。

```bash
npm install
npm run dev
```

## 打包

```bash
npm run build
```

产物在：

- `src-tauri/target/release/bundle/dmg/`
- `src-tauri/target/release/bundle/macos/SocksNode.app`

## 安全说明

- 局域网接入（绑定 `0.0.0.0`）会向同网段暴露一个**无认证**的 SOCKS5 代理，
  请仅在可信网络中开启；界面中也会给出提示。
- 「跳过证书验证」默认开启并向内核传 `-insecure`；关闭后将校验上游证书，
  自签名节点可能因此无法连接。
- 上游密码通过子进程环境变量 `SOCKSNODE_PASSWORD` 传入，不出现在命令行
  参数里（避免被同机进程通过 `ps` 读取）。
- 打包脚本 `scripts/sign-dmg.sh` 只临时添加自签证书，并在退出时还原用户
  原有的钥匙串搜索列表。

## 行为说明

- 「运行中」= 本地 SOCKS 已监听；真正连通请点「测试」
- 关闭窗口 → 隐藏到菜单栏托盘，并隐藏 Dock 图标
- 托盘可：显示主窗口 / 全部启动 / 全部停止 / 退出

## 静态检查

源码自检命令（不访问实际代理节点）：

```bash
npm run build:web
cd src-tauri && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked
cd ../third_party/anytls-go && go test ./... && go vet ./...
```

Rust/TypeScript/Go 静态检查不能代替真实 AnyTLS 连接、SOCKS5 UDP、物理网卡绑定、菜单栏与 DMG 签名的端到端验证。
