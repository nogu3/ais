# ais

[![CI](https://github.com/nogu3/ais/actions/workflows/ci.yml/badge.svg)](https://github.com/nogu3/ais/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

English: [README.en.md](README.en.md)

Panasonic **AiSEG2** 専用 CLI。AiSEG2 の Web UI を HTTP（Digest 認証）で叩き、電力データの読み取りと、AiSEG2 経由でしか操作できない機器（リンクプラス照明等）の制御を行う。

- stdout には**純粋な構造化 JSON のみ**を出力する（`jq` / LLM Function Calling がそのまま食える）
- 診断は stderr に構造化ログ
- ステートレス / one-shot（デーモン化・ポーリングループなし）

cron / n8n / ダッシュボード等の呼び出し側から、`PATH` 上のバイナリとして叩かれる想定。

## 対象ファームウェア

AiSEG2 は 2026 年 9 月生産終了・以後ファーム更新なしのため、HTML / 制御エンドポイントの契約は**凍結**する前提。

> **注意**: 現時点の解釈層は公開リバースエンジニアリング実装（複数の OSS / 技術記事）からのクロス検証で構築しており、**実機での検証はまだ行っていない**。実機検証後、対象ファームウェアバージョンをここに記載して凍結する。
> フィクスチャ（`tests/fixtures/`）も同様に公開情報から再構成したサニタイズ済みのもの。実機から取得したサニタイズ済み HTML への差し替えを推奨（手順は後述）。

## インストール

[GitHub Releases](https://github.com/nogu3/ais/releases) からビルド済みバイナリ（Linux / macOS × x86_64 / arm64、Linux は musl 静的リンク）を取得して `PATH` に置く:

```bash
tar xzf ais-v*-x86_64-unknown-linux-musl.tar.gz
install -m 755 ais-v*/ais ~/.local/bin/
```

またはソースから:

```bash
cargo install --path .
```

## 設定

クレデンシャルは引数または環境変数で渡す。設定ファイルは持たない。

| 環境変数 | 引数 | 既定値 | 説明 |
|---|---|---|---|
| `AISEG_HOST` | `--host` | （必須） | AiSEG2 の IP / ホスト名（例: `192.0.2.16`） |
| `AISEG_USER` | `--user` | `aiseg` | ユーザー名 |
| `AISEG_PASS` | `--pass` | （必須） | パスワード。シェル履歴に残さないため env を推奨 |
| — | `--timeout` | `10` | HTTP タイムアウト秒 |

## 使い方

```bash
export AISEG_HOST=192.0.2.16
export AISEG_PASS=********

# 瞬時電力（太陽光発電 / 買電 / 電気使用量）
ais power
# {"generation_kw":0.5,"usage_kw":1.2,"buy_kw":0.7,"grid_direction":"buy","sources":[{"name":"太陽光","power_w":512}]}

# 分電盤（主幹 + 分岐回路、瞬時値・消費の大きい順）
ais circuits
# [{"name":"主幹","power_w":1200,"kind":"main"},{"name":"リビング エアコン","power_w":650,"kind":"branch"},...]

# 機器コントロール一覧（何を制御できるかの真実は AiSEG2 が持つ）
ais devices
# [{"id":"1073741825:0x029101","name":"リビング照明","kind":"照明","state":"on",...}]

# 照明の on / off（名前・id・nodeId のいずれかで指定）
ais on リビング照明
ais off 1073741825:0x029101

# 積算電力量（本日の発電 / 消費 / 買電 / 売電 kWh）
ais energy
# {"generation_kwh":8.4,"usage_kwh":18.2,"buy_kwh":10.9,"sell_kwh":1.1}

# 回路別の本日 kWh も含める（回路数ぶんリクエストが増える）
ais energy --circuits
# {...,"circuits":[{"id":"30","name":"リビング エアコン","kwh":3.0},...]}

# 日付指定（※実機未検証・公開情報からの推定）
ais energy --date 2026-06-09

# 読み用エスケープハッチ: 任意ページの id 付き要素テキストを抽出
ais fetch /page/graph/51111
# {"path":"/page/graph/51111","values":{"val_kwh":"12.3"}}

# jq との組み合わせ
ais circuits | jq '[.[] | select(.kind=="branch")] | max_by(.power_w)'
```

### 出力スキーマ

**`ais power`**

| フィールド | 型 | 説明 |
|---|---|---|
| `generation_kw` | number | 総発電電力 [kW] |
| `usage_kw` | number | 総使用電力 [kW] |
| `buy_kw` | number | 買電電力 [kW]。売電中は `0`（売電値の出力方法は保留事項） |
| `grid_direction` | string | `"buy"` \| `"sell"` |
| `sources[]` | array | 発電ソース内訳（`name`, `power_w`） |

**`ais circuits`** — 配列。先頭が主幹（`kind: "main"`）、以降が分岐回路（`kind: "branch"`、消費電力降順）。`power_w` が `null` の回路は計測なし。AiSEG2 の表示仕様上、0W の回路以降は省略されることがある。

**`ais energy`**

| フィールド | 型 | 説明 |
|---|---|---|
| `date` | string | 対象日（`--date` 指定時のみ。省略時は本日分で、フィールド自体が出ない） |
| `generation_kwh` | number | 発電量 [kWh] |
| `usage_kwh` | number | 消費量 [kWh] |
| `buy_kwh` | number | 買電量 [kWh] |
| `sell_kwh` | number | 売電量 [kWh] |
| `circuits[]` | array | `--circuits` 指定時のみ。回路別積算（`id`, `name`, `kwh`） |

> 本日分の総計・回路別は公開実装 2 件で検証済み。**`--date` の日付指定パラメータは公開情報からの推定で実機未検証**（動かない場合は exit 6 になる）。月次積算はエンドポイント未確認のため未対応。

**`ais devices`** — 配列。`id`（`<nodeId>:<eoj>`）、`name`、`kind`（AiSEG2 のパネル種別名）、`state`（`"on"` / `"off"`）、`node_id`、`eoj`、`type`、`link`（制御ページ ID）。

**`ais on` / `ais off`** — `{"id","name","kind","requested","result","confirmed","changed"}`。`confirmed` は AiSEG2 側の完了確認（acceptId ポーリング）まで取れたか。`changed` はリクエストを送ったか（トグル系機器で既に希望状態だった場合 `false`）。

### 制御の対象と方式

| 種別 | 方式 | 検証状況 |
|---|---|---|
| 照明（リンクプラス） | `device:{onoff: 0x30/0x31}`（希望状態を送る） | 公開実装 2 件で確認 |
| エアイー / 空気清浄機等 | `state: <現在状態>`（送ると AiSEG2 側で反転） | 類似機器（エアコン/床暖房）の公開実装から推定。**実機での要検証** |

シャッター/カーテン・電気錠・給湯系・EV などは対象外（設計方針は `CLAUDE.md` 参照）。

## exit code

| Code | 意味 | stderr の `kind` |
|---|---|---|
| 0 | 成功 | — |
| 2 | CLI 引数エラー（clap） | — |
| 3 | ネットワーク / タイムアウト | `network` / `timeout` |
| 4 | 認証失敗（401） | `auth_failed` |
| 5 | AiSEG2 が想定外 HTTP ステータスを返した | `http_status` |
| 6 | パース失敗（セレクタ不一致 = **ファームがずれた可能性**） | `parse_failed` |
| 7 | 制御リジェクト / 結果未確認 | `control_rejected` |
| 11 | 指定機器が機器コントロール一覧に見つからない / 複数一致 | `device_not_found` / `device_ambiguous` |

エラーは stderr に 1 行 JSON で出る:

```json
{"error":{"kind":"parse_failed","detail":"no circuit entries found on electricflow/1113 (firmware mismatch?)"}}
```

## アーキテクチャ

```
src/
├── main.rs        # CLI（clap）・コマンドのオーケストレーション・exit code
├── error.rs       # kind → exit code / stderr JSON
├── fetch/         # フェッチ層: HTTP + 自前 Digest 認証（ureq）。中身は解釈しない
│   └── digest.rs
├── parse/         # 解釈層: ファーム依存の壊れやすい契約をここに閉じ込める
│   ├── power.rs     # POST /data/electricflow/111/update (JSON)
│   ├── circuits.rs  # GET /page/electricflow/1113?id=N (HTML)
│   ├── energy.rs    # GET /page/graph/5x111・584・回路カタログ 734 (HTML)
│   ├── devices.rs   # 機器コントロール一覧の走査 (HTML)
│   └── generic.rs   # ais fetch 用の汎用抽出
└── control.rs     # 解釈層: 制御ペイロード生成・change/check レスポンス解釈
```

ファームで構造が変わった場合は `parse/` と `control.rs` だけを直す。

## 開発

[Task](https://taskfile.dev) を使う場合:

```bash
task build      # デバッグビルド
task test       # フィクスチャに対する解釈層テスト（実機不要）
task check      # fmt チェック + clippy + テスト（push 前はこれ）
task e2e        # モック AiSEG2 への E2E スモーク（Digest 認証込み・実機不要）
task run -- power   # RUST_LOG=debug で実行
```

素の cargo でも同じことができる:

```bash
cargo build
cargo test
cargo clippy -- -D warnings
RUST_LOG=debug cargo run -- power   # 診断ログは stderr へ
```

`task e2e` は `tools/mock-aiseg2.py`（Digest 認証付きモック AiSEG2、フィクスチャを配信）を 127.0.0.1:18080 に立てて、全サブコマンドと exit code 規約を検証する。

### リリース

`v*` タグを push すると GitHub Actions がクロスビルドして Releases にバイナリ（tar.gz + SHA256SUMS）を公開する:

```bash
git tag v0.1.0
git push origin v0.1.0
```

対象: `x86_64-unknown-linux-musl` / `aarch64-unknown-linux-musl` / `x86_64-apple-darwin` / `aarch64-apple-darwin`。
AiSEG2 は LAN 内の平文 HTTP のみのため TLS をリンクしない（Linux は完全静的バイナリ）。

### 実機 E2E 手順（CI には載せない）

1. AiSEG2 と同一 LAN のマシンで `AISEG_HOST` / `AISEG_PASS` を設定する。
2. 読み: `ais power` → `ais circuits` → `ais energy` → `ais energy --circuits` → `ais devices` の順に実行し、JSON が返ること・値が AiSEG2 の画面表示と一致することを確認する。`ais energy --date <昨日>` も実行し、日付指定パラメータが効くか検証する（結果を README に反映）。
3. 制御: `ais devices` で対象照明の `id` を確認 → `ais on <id>` → 実灯確認 → `ais off <id>`。
4. exit code 6 が出た場合はファームの構造が想定とずれている。`RUST_LOG=debug` で対象ページを特定し、`ais fetch <page>` で実構造を確認する。

### フィクスチャの取得とサニタイズ

実機からフィクスチャを更新する場合:

```bash
curl --digest -u "aiseg:$AISEG_PASS" "http://$AISEG_HOST/page/electricflow/1113?id=1&request_by_form=1" -o page1.html
```

コミット前に必ず以下をサニタイズする（**パブリックリポジトリのため厳守**）:

- 実機の IP・ホスト名 → `192.0.2.0/24`（RFC 5737）のダミーに置換
- 実在の回路名・部屋名・機器名 → 一般的な名称に置換
- nodeId / eoj / token / acceptId → 架空の値に置換
- ユーザー名・パスワード・シリアル番号類 → 削除

## ライセンス

MIT
