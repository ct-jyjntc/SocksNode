# anytls-client UDP fix

Bundled kernel: `anytls/0.0.13-socksnode5`

## Problem

Official `anytls-client` accepts SOCKS5 `UDP ASSOCIATE`, but opens the
UDP-over-TCP stream using the associate bind address (`0.0.0.0:0`).

Many server stacks (sing-box / panel routes) use that UoT request destination
for routing/ACL at handshake time. `0.0.0.0:0` is then matched to
`outbound/block`, so UDP fails while TCP still works.

mihomo works because it creates the anytls UoT stream with the **real packet
destination** (e.g. `8.8.8.8:53`).

## Fix

In `cmd/client/inbound.go` `NewPacketConnection`:

1. Wait for the first UDP packet from the local SOCKS association
2. Open the anytls UoT stream with that real destination
3. Replay the first packet via `bufio.NewCachedPacketConn`

## Other SocksNode patches

`third_party/anytls-go/` is a fork snapshot of the kernel used to build the
bundled binary. On top of the UDP fix it also adds:

- `-udp=false` — reject UDP when a node disables UDP.
- `-insecure` (default `false`) — opt-in TLS certificate verification skip,
  driven by the per-node `skipCertVerify` setting.
- `-bind <ipv4>` / `-iface <name>` — force outbound dials onto a physical NIC
  (see `proxy/system_dialer.go`), used to avoid Surge/Clash `utun` + fake-ip.
- `SOCKSNODE_PASSWORD` env var — password fallback so it does not appear in
  `argv` / `ps` output.

Rebuild the vendored source with the command below to get a binary that
matches `src-tauri/resources/anytls-client`.

## Rebuild

```bash
# Run from the SocksNode repository root.
cd third_party/anytls-go
go build -trimpath -ldflags '-s -w' -o ../../src-tauri/resources/anytls-client ./cmd/client
```

## Verify

```bash
# TCP
curl --socks5-hostname 127.0.0.1:5001 https://api.ipify.org

# UDP ASSOCIATE + DNS
# should return a DNS response, not time out
```
