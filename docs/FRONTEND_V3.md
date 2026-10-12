# SocksNode 前端 v3 重构

参考：用户提供的 v3.html（magpie 风格）。

## 实现

- 视觉：复用原型的全局色彩变量、顶部居中分段导航、节点分行列表、行内编辑器、弹窗、卡片、亮暗主题、微动效与响应式布局。
- 主导航：节点、网关、优选、延迟、日志（共 5 页），另有设置入口。原「上游」页已并入「节点」。数据源全部通过 src/lib/api.ts 调用 Tauri。
- 节点：实时状态/端口/IP、单节点启停、全部启停、运行节点测延迟；每行右侧箭头展开行内编辑器（编辑、删除、复制 anytls:// 链接），「添加节点」支持 AnyTLS URI 解析、保存并启动。
- 网关：本机 SOCKS5 连接命令、局域网 SOCKS（如可用）。
- 优选：真实上游 IP、最后探测、当前策略、重新优选；连接线沿用原型布局。
- 延迟：基于 lastTest / lastProbe 的当前快照与逐节点测试。
- 日志：通过 nodes:log 事件与 nodes_logs 接口更新，分类过滤。
- 设置：原有系统登录项、自动启动、测速、探活、iLatency 测试与 Cookie 配置及主题。

## 与演示 HTML 的有意差异

原型中的国家、节点、历史走势图、候选 IP 排名、连接数和速度均为示意数据。生产前端不引入这些假数据：

1. 后端没有公开 DNS 多候选详细清单，因此优选页面只显示 activeIp/lastProbe，不允许伪造手动钉 IP。
2. 后端没有历史延迟时序，因此延迟页不显示虚构的 7/30 日图表。
3. 导入支持 anytls:// 链接，未添加尚无后端支持的批量文件导入。
4. 「节点运行中」是本机 SOCKS 端口就绪，不等于已验证真实网络出口。
5. 旧文件已删除：styles/index.css、components/ui.tsx、components/WindowControls.tsx、lib/utils.ts 及其依赖 clsx、tailwind-merge、sonner；tailwindcss 与 @tailwindcss/vite 插件也已移除。正式入口只加载 styles/v3.css。
6. 源 v3.html 没有提交进应用构建，也没有保留其模拟节点和模拟日志。

## 开发验证

在项目根目录运行：

    ./node_modules/.bin/tsc --noEmit -p tsconfig.json
    npm run build:web

原生 Tauri 的窗口/菜单栏、真实 AnyTLS 连接与测速仍需在 macOS 实机验证。
