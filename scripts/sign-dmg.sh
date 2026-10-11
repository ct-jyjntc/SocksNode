#!/usr/bin/env bash
# 自签 SocksNode.app 并打成可分发 DMG（无需 Apple Developer ID）。
# 别人电脑：拖到「应用程序」后，双击「首次打开.command」或右键 App → 打开。
# 未公证，Gatekeeper 会提示一次，不是「已损坏」。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="${APP:-$ROOT/src-tauri/target/release/bundle/macos/SocksNode.app}"
OUT_DIR="${OUT_DIR:-$ROOT}"
VERSION="$(python3 -c "import json; print(json.load(open('$ROOT/src-tauri/tauri.conf.json'))['version'])")"
OUT_DMG="$OUT_DIR/SocksNode-${VERSION}-arm64.dmg"
IDENTITY_NAME="SocksNode Developer"
LOGIN_KC="$HOME/Library/Keychains/login.keychain-db"
TMP="$(mktemp -d /tmp/socksnode-sign.XXXXXX)"

cleanup() {
  if [[ ${#ORIG_KEYCHAINS[@]} -gt 0 ]]; then
    security list-keychains -d user -s "${ORIG_KEYCHAINS[@]}" >/dev/null 2>&1 || true
  fi
  if [[ -d "$TMP" ]]; then
    find "$TMP" -mindepth 1 -maxdepth 1 -exec rm -rf {} + 2>/dev/null || true
    rmdir "$TMP" 2>/dev/null || true
  fi
}
# Capture the user's original keychain search list so cleanup can restore it
# exactly (adding a temporary keychain must not drop iCloud/custom keychains).
ORIG_KEYCHAINS=()
while read -r kc; do
  kc="${kc%\"}"
  kc="${kc#\"}"
  ORIG_KEYCHAINS+=("$kc")
done < <(security list-keychains -d user)

trap cleanup EXIT


if [[ ! -d "$APP" ]]; then
  echo "error: app not found: $APP" >&2
  echo "run: npm run build" >&2
  exit 1
fi

echo "[cert] creating ephemeral codesign identity"
cat > "$TMP/codesign.cnf" <<'EOF'
[req]
distinguished_name = req_distinguished_name
x509_extensions = v3_codesign
prompt = no
[req_distinguished_name]
CN = SocksNode Developer
OU = SocksNode
O = SocksNode
C = CN
[v3_codesign]
basicConstraints = CA:FALSE
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
subjectKeyIdentifier = hash
EOF
openssl req -new -x509 -days 3650 -nodes \
  -newkey rsa:2048 \
  -keyout "$TMP/sign.key" \
  -out "$TMP/sign.crt" \
  -config "$TMP/codesign.cnf" >/dev/null 2>&1
openssl pkcs12 -export \
  -legacy \
  -inkey "$TMP/sign.key" \
  -in "$TMP/sign.crt" \
  -out "$TMP/sign.p12" \
  -passout pass:socksnode \
  -name "$IDENTITY_NAME" >/dev/null 2>&1

KC="$TMP/sign.keychain-db"
security create-keychain -p socksnode "$KC"
security set-keychain-settings -t 3600 -u "$KC"
security unlock-keychain -p socksnode "$KC"
security import "$TMP/sign.p12" -k "$KC" -P socksnode -T /usr/bin/codesign -T /usr/bin/security >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k socksnode "$KC" >/dev/null
security add-trusted-cert -d -r trustRoot -p codeSign -k "$KC" "$TMP/sign.crt"
if [[ ${#ORIG_KEYCHAINS[@]} -gt 0 ]]; then
  security list-keychains -d user -s "$KC" "${ORIG_KEYCHAINS[@]}"
else
  security list-keychains -d user -s "$KC"
fi

if ! security find-identity -v -p codesigning "$KC" | grep -q "$IDENTITY_NAME"; then
  echo "error: identity not visible" >&2
  security find-identity -v -p codesigning "$KC" || true
  exit 1
fi

# 不要 --options runtime：未公证 + hardened runtime 在别人电脑上常报「已损坏」。
sign() {
  echo "[sign] $1"
  codesign --force --sign "$IDENTITY_NAME" --keychain "$KC" --timestamp=none "$1"
}

HELPER="$APP/Contents/Resources/resources/anytls-client"
[[ -f "$HELPER" ]] && sign "$HELPER"
sign "$APP/Contents/MacOS/socks-node"
sign "$APP"

echo "[verify] app"
codesign --verify --verbose=2 "$APP"
codesign -dv --verbose=2 "$APP" 2>&1 | sed -n '1,14p'

STAGE="$TMP/dmgroot"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/SocksNode.app"
ln -s /Applications "$STAGE/Applications"

cat > "$STAGE/安装说明.txt" <<'EOF'
SocksNode 安装说明（Apple Silicon）

1. 把 SocksNode 拖到「应用程序」文件夹
2. 第一次打开请用下面任一方式（只需一次）：

   A. 双击「首次打开.command」
   B. 按住 Control 点按 SocksNode → 打开 → 打开
   C. 系统设置 → 隐私与安全性 → 仍要打开

说明：本包为开发者自签，未经 Apple 公证，
系统会提示「未识别的开发者」，不是病毒，也不是损坏。
EOF

cat > "$STAGE/首次打开.command" <<'EOF'
#!/bin/bash
# 去掉浏览器下载带来的隔离属性，然后启动 App。
set -e
APP=""
if [[ -d "/Applications/SocksNode.app" ]]; then
  APP="/Applications/SocksNode.app"
elif [[ -d "$(dirname "$0")/SocksNode.app" ]]; then
  APP="$(cd "$(dirname "$0")" && pwd)/SocksNode.app"
fi
if [[ -z "$APP" ]]; then
  osascript -e 'display dialog "请先把 SocksNode 拖到「应用程序」文件夹，再运行本脚本。" buttons {"好"} default button 1'
  exit 1
fi
xattr -cr "$APP" || true
open "$APP"
EOF
chmod +x "$STAGE/首次打开.command"

echo "[dmg] $OUT_DMG"
rm -f "$OUT_DMG" "$TMP/rw.dmg"
hdiutil create -volname "SocksNode" -srcfolder "$STAGE" -ov -format UDRW "$TMP/rw.dmg" >/dev/null
hdiutil convert "$TMP/rw.dmg" -format UDZO -imagekey zlib-level=9 -o "$OUT_DMG" >/dev/null
sign "$OUT_DMG"

echo "[verify] dmg"
codesign --verify --verbose=2 "$OUT_DMG"
codesign -dv --verbose=2 "$OUT_DMG" 2>&1 | sed -n '1,10p'
ls -lh "$OUT_DMG"
echo "ok: $OUT_DMG"
