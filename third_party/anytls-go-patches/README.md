# anytls-client UDP fix

Bundled kernel: `anytls/0.0.13-socksnode1`

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

## Rebuild

```bash
git clone https://github.com/anytls/anytls-go.git
cd anytls-go
git apply /path/to/0001-fix-socks5-udp-associate-destination.patch
go build -trimpath -ldflags '-s -w' -o anytls-client ./cmd/client
cp anytls-client /path/to/ScoksNode/src-tauri/resources/anytls-client
```

## Verify

```bash
# TCP
curl --socks5-hostname 127.0.0.1:5001 https://api.ipify.org

# UDP ASSOCIATE + DNS
# should return a DNS response, not time out
```
