#!/usr/bin/env bash
# モック AiSEG2（tools/mock-aiseg2.py）に対する E2E スモーク。
# Digest 認証・全サブコマンド・exit code 規約を実機なしで検証する。
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --quiet

python3 tools/mock-aiseg2.py >/dev/null 2>&1 &
MOCK_PID=$!
trap 'kill "$MOCK_PID" 2>/dev/null || true' EXIT
sleep 1

export AISEG_HOST=127.0.0.1:18080 AISEG_USER=aiseg AISEG_PASS=secret
AIS=./target/debug/ais

fail=0
expect() { # <説明> <期待 exit code> <コマンド...>
  local desc=$1 want=$2 got
  shift 2
  set +e
  "$@" >/dev/null 2>&1
  got=$?
  set -e
  if [ "$got" -eq "$want" ]; then
    echo "ok   ${desc}"
  else
    echo "FAIL ${desc} (exit ${got}, want ${want})"
    fail=1
  fi
}

expect "power"             0  "$AIS" power
expect "circuits"          0  "$AIS" circuits
expect "devices"           0  "$AIS" devices
expect "on (名前指定)"     0  "$AIS" on "リビング照明"
expect "off (id 指定)"     0  "$AIS" off "1073741825:0x029101"
expect "fetch"             0  "$AIS" fetch "/page/devices/device/32i1?page=1"
expect "機器未発見 -> 11"  11 "$AIS" on "存在しない機器"
expect "認証失敗 -> 4"     4  env AISEG_PASS=wrong "$AIS" power
expect "引数エラー -> 2"   2  env -u AISEG_HOST "$AIS" power

# energy はブランチによって未実装の場合があるため存在チェックしてから
if "$AIS" energy --help >/dev/null 2>&1; then
  expect "energy"           0 "$AIS" energy
  expect "energy --circuits" 0 "$AIS" energy --circuits
fi

exit "$fail"
