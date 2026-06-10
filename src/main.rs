use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

use clap::{CommandFactory, Parser, Subcommand};
use serde::Serialize;
use tracing::debug;
use tracing_subscriber::EnvFilter;

use ais::control::{self, CheckStatus};
use ais::error::{AisError, ErrorKind, Result};
use ais::fetch::Client;
use ais::parse::{circuits, devices, energy, generic, power};

/// Panasonic AiSEG2 専用 CLI。stdout には構造化 JSON のみを出力する。
#[derive(Parser)]
#[command(name = "ais", version, about)]
struct Cli {
    /// AiSEG2 のホスト名 / IP（例: 192.0.2.16）
    #[arg(long, env = "AISEG_HOST", global = true)]
    host: Option<String>,

    /// AiSEG2 のユーザー名
    #[arg(long, env = "AISEG_USER", default_value = "aiseg", global = true)]
    user: String,

    /// AiSEG2 のパスワード（シェル履歴に残さないため環境変数を推奨）
    #[arg(long, env = "AISEG_PASS", hide_env_values = true, global = true)]
    pass: Option<String>,

    /// HTTP タイムアウト秒
    #[arg(long, default_value_t = 10, global = true)]
    timeout: u64,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 瞬時電力（太陽光発電 / 買電 / 電気使用量）を JSON で出力する
    Power,
    /// 分電盤（主幹 + 分岐回路）の瞬時電力を JSON 配列で出力する
    Circuits,
    /// 任意ページを取得し、id 属性付き要素のテキストを JSON で出力する（読み用エスケープハッチ）
    Fetch {
        /// ページパス（例: /page/graph/51111）
        page: String,
    },
    /// 積算電力量（発電 / 消費 / 買電 / 売電 kWh）を JSON で出力する
    Energy {
        /// 対象日（YYYY-MM-DD）。省略時は本日。※日付指定は実機未検証
        #[arg(long, value_parser = parse_date)]
        date: Option<(u32, u32, u32)>,

        /// 回路別の積算 kWh も含める（回路数ぶんリクエストが増える）
        #[arg(long)]
        circuits: bool,
    },
    /// 機器コントロール一覧（制御可能機器とその状態）を JSON で出力する
    Devices,
    /// 機器を ON にする（名前または id で指定）
    On { device: String },
    /// 機器を OFF にする（名前または id で指定）
    Off { device: String },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let (host, pass) = match (&cli.host, &cli.pass) {
        (Some(h), Some(p)) => (h.clone(), p.clone()),
        (None, _) => missing_arg("--host (or AISEG_HOST)"),
        (_, None) => missing_arg("--pass (or AISEG_PASS)"),
    };

    let client = Client::new(&host, &cli.user, &pass, cli.timeout);

    match run(&client, &cli.command) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}", e.to_json());
            ExitCode::from(e.kind.exit_code() as u8)
        }
    }
}

fn missing_arg(name: &str) -> ! {
    // exit code 2（CLI 引数エラー）は clap に合わせる
    Cli::command()
        .error(
            clap::error::ErrorKind::MissingRequiredArgument,
            format!("{name} is required"),
        )
        .exit()
}

fn run(client: &Client, command: &Command) -> Result<String> {
    match command {
        Command::Power => to_json(&cmd_power(client)?),
        Command::Circuits => to_json(&cmd_circuits(client)?),
        Command::Fetch { page } => to_json(&cmd_fetch(client, page)?),
        Command::Energy { date, circuits } => to_json(&cmd_energy(client, *date, *circuits)?),
        Command::Devices => to_json(&cmd_devices(client)?),
        Command::On { device } => to_json(&cmd_control(client, device, true)?),
        Command::Off { device } => to_json(&cmd_control(client, device, false)?),
    }
}

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|e| AisError::parse_failed(format!("failed to serialize output: {e}")))
}

const ELECTRICFLOW_UPDATE_PATH: &str = "/data/electricflow/111/update";

fn cmd_power(client: &Client) -> Result<power::Power> {
    let body = client.post_form(ELECTRICFLOW_UPDATE_PATH, "")?;
    power::parse_power(&body)
}

#[derive(Serialize)]
struct CircuitRow {
    name: String,
    power_w: Option<i64>,
    kind: &'static str,
}

/// 回路ページの走査上限。AiSEG2 の計測回路は最大でも数十なので安全弁として置く。
const CIRCUIT_PAGE_LIMIT: u32 = 10;

fn cmd_circuits(client: &Client) -> Result<Vec<CircuitRow>> {
    // 主幹は電力フローの総使用電力を使う
    let body = client.post_form(ELECTRICFLOW_UPDATE_PATH, "")?;
    let p = power::parse_power(&body)?;

    let mut rows = vec![CircuitRow {
        name: "主幹".to_string(),
        power_w: Some((p.usage_kw * 1000.0).round() as i64),
        kind: "main",
    }];

    // 分岐回路は消費電力降順でページングされる。0W 以降は省略される仕様
    let mut prev: Vec<circuits::Circuit> = Vec::new();
    for page in 1..=CIRCUIT_PAGE_LIMIT {
        let html = client.get(&format!(
            "/page/electricflow/1113?id={page}&request_by_form=1"
        ))?;
        let entries = circuits::parse_circuit_page(&html);
        debug!(page, count = entries.len(), "parsed circuit page");

        if page == 1 && entries.is_empty() {
            return Err(AisError::parse_failed(
                "no circuit entries found on electricflow/1113 (firmware mismatch?)",
            ));
        }
        if entries.is_empty() || entries == prev {
            break;
        }

        let reached_zero = entries.iter().any(|c| c.power_w.unwrap_or(0) == 0);

        rows.extend(entries.iter().map(|c| CircuitRow {
            name: c.name.clone(),
            power_w: c.power_w,
            kind: "branch",
        }));

        if reached_zero {
            break;
        }
        prev = entries;
    }

    Ok(rows)
}

fn cmd_fetch(client: &Client, page: &str) -> Result<generic::PageValues> {
    let path = if page.starts_with('/') {
        page.to_string()
    } else {
        format!("/{page}")
    };
    let html = client.get(&path)?;
    Ok(generic::extract_page_values(&path, &html))
}

/// `--date` の YYYY-MM-DD を (年, 月, 日) にパースする（clap が exit 2 で扱う）。
fn parse_date(s: &str) -> std::result::Result<(u32, u32, u32), String> {
    let parts: Vec<&str> = s.split('-').collect();
    let err = || format!("invalid date '{s}' (expected YYYY-MM-DD)");
    if parts.len() != 3 {
        return Err(err());
    }
    let y: u32 = parts[0].parse().map_err(|_| err())?;
    let m: u32 = parts[1].parse().map_err(|_| err())?;
    let d: u32 = parts[2].parse().map_err(|_| err())?;
    if !(2000..=2099).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(err());
    }
    Ok((y, m, d))
}

#[derive(Serialize)]
struct EnergyReport {
    /// 対象日（YYYY-MM-DD）。省略時（本日）は出さない
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
    generation_kwh: f64,
    usage_kwh: f64,
    buy_kwh: f64,
    sell_kwh: f64,
    /// --circuits 指定時のみ
    #[serde(skip_serializing_if = "Option::is_none")]
    circuits: Option<Vec<CircuitEnergy>>,
}

#[derive(Serialize)]
struct CircuitEnergy {
    id: String,
    name: String,
    kwh: f64,
}

fn cmd_energy(
    client: &Client,
    date: Option<(u32, u32, u32)>,
    with_circuits: bool,
) -> Result<EnergyReport> {
    let fetch_kwh = |page_id: u32| -> Result<f64> {
        let html = client.get(&energy::graph_path(page_id, None, date))?;
        energy::parse_val_kwh(&html)
    };

    let generation_kwh = fetch_kwh(energy::GRAPH_GENERATION_DAY)?;
    let usage_kwh = fetch_kwh(energy::GRAPH_USAGE_DAY)?;
    let buy_kwh = fetch_kwh(energy::GRAPH_BUY_DAY)?;
    let sell_kwh = fetch_kwh(energy::GRAPH_SELL_DAY)?;

    let circuits = if with_circuits {
        let catalog_html = client.get(energy::CIRCUIT_CATALOG_PATH)?;
        let catalog = energy::parse_circuit_catalog(&catalog_html)?;
        debug!(count = catalog.len(), "found measured circuits in catalog");

        let mut rows = Vec::with_capacity(catalog.len());
        for circuit in catalog {
            let html = client.get(&energy::graph_path(
                energy::GRAPH_CIRCUIT_DAY,
                Some(&circuit.id),
                date,
            ))?;
            let kwh = energy::parse_val_kwh(&html)?;
            rows.push(CircuitEnergy {
                id: circuit.id,
                name: circuit.name,
                kwh,
            });
        }
        Some(rows)
    } else {
        None
    };

    Ok(EnergyReport {
        date: date.map(|(y, m, d)| format!("{y:04}-{m:02}-{d:02}")),
        generation_kwh,
        usage_kwh,
        buy_kwh,
        sell_kwh,
        circuits,
    })
}

/// 機器ページの 1 ページあたりの最大表示数（これに満たなければ最終ページ）。
const DEVICES_PER_PAGE: usize = 8;
/// 機器ページの走査上限（安全弁）。
const DEVICE_PAGE_LIMIT: u32 = 10;

fn cmd_devices(client: &Client) -> Result<Vec<devices::Device>> {
    enumerate_devices(client)
}

fn enumerate_devices(client: &Client) -> Result<Vec<devices::Device>> {
    let index = client.get("/")?;
    let menu_link = devices::parse_devices_menu_link(&index).ok_or_else(|| {
        AisError::parse_failed("devices menu link not found on top page (firmware mismatch?)")
    })?;

    let panels_html = client.get(&menu_link)?;
    let panels = devices::parse_panels(&panels_html);
    if panels.is_empty() {
        return Err(AisError::parse_failed(
            "no device panels found on device control page (firmware mismatch?)",
        ));
    }
    debug!(count = panels.len(), "found device panels");

    let mut all = Vec::new();
    for panel in &panels {
        let page_id = devices::panel_page_id(&panel.link).to_string();
        let separator = if panel.link.contains('?') { '&' } else { '?' };

        let mut prev_page: Vec<devices::Device> = Vec::new();
        for page in 1..=DEVICE_PAGE_LIMIT {
            let path = format!(
                "/page/devices/device/{}{}individual_page={}",
                panel.link, separator, page
            );
            let html = client.get(&path)?;
            let token = devices::parse_token(&html).unwrap_or_default();
            let mut found = devices::parse_device_panels(&html);
            debug!(
                kind = panel.kind,
                page,
                count = found.len(),
                "parsed device page"
            );

            if found.is_empty() || found == prev_page {
                break;
            }
            let last_page = found.len() < DEVICES_PER_PAGE;
            prev_page = found.clone();

            for d in &mut found {
                d.kind = panel.kind.clone();
                d.link = page_id.clone();
                d.token = token.clone();
            }
            all.extend(found);

            if last_page {
                break;
            }
        }
    }

    Ok(all)
}

#[derive(Serialize)]
struct ControlResult {
    id: String,
    name: String,
    kind: String,
    requested: &'static str,
    result: &'static str,
    /// AiSEG2 側で完了確認まで取れたか（acceptId なしの同期応答も true）
    confirmed: bool,
    /// 実際に制御リクエストを送ったか（既に希望状態だった場合 false）
    changed: bool,
}

const CHECK_POLL_MAX: u32 = 6;
const CHECK_POLL_INTERVAL: Duration = Duration::from_secs(1);

fn cmd_control(client: &Client, query: &str, on: bool) -> Result<ControlResult> {
    let all = enumerate_devices(client)?;

    let matches: Vec<&devices::Device> = all
        .iter()
        .filter(|d| d.id == query || d.name == query || d.node_id == query)
        .collect();

    let device = match matches.len() {
        0 => {
            return Err(AisError::new(
                ErrorKind::DeviceNotFound,
                format!("device not found in control list: {query}"),
            ))
        }
        1 => matches[0],
        _ => {
            let ids: Vec<&str> = matches.iter().map(|d| d.id.as_str()).collect();
            return Err(AisError::new(
                ErrorKind::DeviceAmbiguous,
                format!(
                    "multiple devices match '{query}': {} (specify by id)",
                    ids.join(", ")
                ),
            ));
        }
    };

    let requested = if on { "on" } else { "off" };
    let is_light = device.kind == "照明";

    // トグル系は「現在状態を送ると反転」のため、既に希望状態なら何もしない
    if !is_light && device.state == requested {
        return Ok(ControlResult {
            id: device.id.clone(),
            name: device.name.clone(),
            kind: device.kind.clone(),
            requested,
            result: "ok",
            confirmed: true,
            changed: false,
        });
    }

    if device.token.is_empty() {
        return Err(AisError::parse_failed(
            "control token not found on device page (firmware mismatch?)",
        ));
    }

    let payload = if is_light {
        control::light_change_payload(&device.token, device, on)
    } else {
        control::toggle_change_payload(&device.token, device)
    };
    let body = format!("data={}", control::urlencode(&payload));
    let change_path = format!("/action/devices/device/{}/change", device.link);
    debug!(path = change_path, payload, "sending control request");

    let response = client.post_form(&change_path, &body)?;
    let accept_id = control::parse_change_response(&response)?;

    let confirmed = match accept_id {
        None => true,
        Some(accept_id) => confirm_control(client, device, &accept_id)?,
    };

    Ok(ControlResult {
        id: device.id.clone(),
        name: device.name.clone(),
        kind: device.kind.clone(),
        requested,
        result: "ok",
        confirmed,
        changed: true,
    })
}

/// acceptId 付きの非同期制御を check エンドポイントで完了確認する。
fn confirm_control(client: &Client, device: &devices::Device, accept_id: &str) -> Result<bool> {
    let check_path = format!("/data/devices/device/{}/check", device.link);
    let body = format!(
        "data={}",
        control::urlencode(&control::check_payload(accept_id, &device.dev_type))
    );

    for attempt in 1..=CHECK_POLL_MAX {
        sleep(CHECK_POLL_INTERVAL);
        let response = client.post_form(&check_path, &body)?;
        match control::parse_check_response(&response)? {
            CheckStatus::Done => return Ok(true),
            CheckStatus::InProgress => {
                debug!(attempt, "control still in progress");
            }
            CheckStatus::Failed(code) => {
                return Err(AisError::new(
                    ErrorKind::ControlRejected,
                    format!("control result check failed (result: {code})"),
                ));
            }
        }
    }

    Err(AisError::new(
        ErrorKind::ControlRejected,
        format!("control result unconfirmed after {CHECK_POLL_MAX} checks (acceptId: {accept_id})"),
    ))
}
