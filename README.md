# SocksNode (Tauri)

把上游代理节点（如 anytls）转发为本地 SOCKS5 的轻量桌面工具。

- 内置 `anytls-client` 内核（约 6MB，替代原 41MB mihomo）
- 已打补丁支持 SOCKS5 UDP（UoT 握手使用真实目标地址，版本 `anytls/0.0.13-socksnode1`）
- 每个节点独立进程
- 仅本地 SOCKS，不启用系统代理 / TUN
- Quiet Console 风格界面
- **Tauri 2** 壳（系统 WebView 壳）

> UDP 补丁说明见 `third_party/anytls-go-patches/`

## 预置节点

| 节点 | 上游 | 本地 SOCKS |
|------|------|------------|
| `香港` | `156.239.13.236:12002` | `127.0.0.1:5001` |
| `美国` | `znjxdm.lajiaoyun.sbs:50001` | `127.0.0.1:5002` |

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

## 行为说明

- 「运行中」= 本地 SOCKS 已监听；真正连通请点「测试」
- 关闭窗口 → 隐藏到菜单栏托盘，并隐藏 Dock 图标
- 托盘可：显示主窗口 / 全部启动 / 全部停止 / 退出
