//! 積算電力量（kWh）ページの解釈。
//!
//! 契約（ファーム凍結前提）:
//! - 本日の総計: `GET /page/graph/51111`（発電）/ `52111`（消費）/ `53111`（買電）/ `54111`（売電）
//!   いずれも `#val_kwh` 要素のテキストが kWh 値
//! - 回路別: `GET /page/graph/584?data=<base64 JSON>`（`{"circuitid":"<id>"}`）→ 同じく `#val_kwh`
//! - 回路カタログ: `GET /page/setting/installation/734` の `<script>` 内
//!   `init({...})` 引数 JSON の `arrayCircuitNameList`（`strBtnType == "1"` が計測対象）
//! - 日付指定: `data` JSON に `{"day":[Y,M,D],"term":"YYYY/MM/DD","termStr":"day"}` を加える
//!   （公開情報からの推定。省略時は本日になることは検証済み）

use scraper::{Html, Selector};
use serde::Serialize;

use crate::error::{AisError, Result};
use crate::parse::lenient_number;

/// 本日（または指定日）の総計 kWh を表示するグラフページの ID。
pub const GRAPH_GENERATION_DAY: u32 = 51111;
pub const GRAPH_USAGE_DAY: u32 = 52111;
pub const GRAPH_BUY_DAY: u32 = 53111;
pub const GRAPH_SELL_DAY: u32 = 54111;
/// 回路別 kWh のグラフページ ID。
pub const GRAPH_CIRCUIT_DAY: u32 = 584;
/// 回路カタログ（計測回路名称一覧）のページパス。
pub const CIRCUIT_CATALOG_PATH: &str = "/page/setting/installation/734";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CatalogCircuit {
    pub id: String,
    pub name: String,
}

/// グラフページから kWh 値（`#val_kwh`）を取り出す。
pub fn parse_val_kwh(html: &str) -> Result<f64> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("#val_kwh").unwrap();
    let el = doc.select(&sel).next().ok_or_else(|| {
        AisError::parse_failed("#val_kwh not found on graph page (firmware mismatch?)")
    })?;
    let text = el.text().collect::<Vec<_>>().join("");
    lenient_number(&text).ok_or_else(|| {
        AisError::parse_failed(format!("#val_kwh is not numeric: {:?}", text.trim()))
    })
}

/// 回路カタログページから計測対象回路（strBtnType == "1"）の一覧を取り出す。
pub fn parse_circuit_catalog(html: &str) -> Result<Vec<CatalogCircuit>> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("script").unwrap();

    let script = doc
        .select(&sel)
        .map(|el| el.text().collect::<String>())
        .find(|t| t.contains("arrayCircuitNameList"))
        .ok_or_else(|| {
            AisError::parse_failed(
                "arrayCircuitNameList script not found on circuit catalog page (firmware mismatch?)",
            )
        })?;

    // `window.onload = init({...});` の引数 JSON を取り出す
    let start = script.find('(');
    let end = script.rfind(')');
    let json_str = match (start, end) {
        (Some(s), Some(e)) if e > s => script[s + 1..e].trim(),
        _ => {
            return Err(AisError::parse_failed(
                "init(...) call not found in circuit catalog script",
            ))
        }
    };
    let v: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| AisError::parse_failed(format!("circuit catalog JSON is invalid: {e}")))?;

    let list = v
        .get("arrayCircuitNameList")
        .and_then(|l| l.as_array())
        .ok_or_else(|| AisError::parse_failed("arrayCircuitNameList is not an array"))?;

    let mut circuits = Vec::new();
    for entry in list {
        if entry.get("strBtnType").and_then(|b| b.as_str()) != Some("1") {
            continue;
        }
        let Some(id) = entry.get("strId").and_then(|i| i.as_str()) else {
            continue;
        };
        let name = entry
            .get("strCircuit")
            .and_then(|n| n.as_str())
            .filter(|n| !n.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("Circuit {id}"));
        circuits.push(CatalogCircuit {
            id: id.to_string(),
            name,
        });
    }
    Ok(circuits)
}

/// グラフページの `data` クエリ用 JSON。`date` は (年, 月, 日)。
pub fn graph_data_json(circuit_id: Option<&str>, date: Option<(u32, u32, u32)>) -> String {
    let mut obj = serde_json::Map::new();
    if let Some(id) = circuit_id {
        obj.insert("circuitid".into(), id.into());
    }
    if let Some((y, m, d)) = date {
        obj.insert("day".into(), serde_json::json!([y, m, d]));
        obj.insert("term".into(), format!("{y:04}/{m:02}/{d:02}").into());
        obj.insert("termStr".into(), "day".into());
    }
    serde_json::Value::Object(obj).to_string()
}

/// グラフページのパスを組み立てる。`data` が空 JSON になる場合はクエリを付けない。
pub fn graph_path(page_id: u32, circuit_id: Option<&str>, date: Option<(u32, u32, u32)>) -> String {
    if circuit_id.is_none() && date.is_none() {
        return format!("/page/graph/{page_id}");
    }
    let data = graph_data_json(circuit_id, date);
    format!("/page/graph/{page_id}?data={}", base64(data.as_bytes()))
}

/// 標準アルファベット・パディングありの base64（依存を増やさないため自前実装）。
pub fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(TABLE[(n >> 18 & 0x3f) as usize] as char);
        out.push(TABLE[(n >> 12 & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6 & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    #[test]
    fn base64_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn graph_data_json_circuit_only_matches_reference_impl() {
        // aiseg2-bridge / AiSEG2-monitor と同じ {"circuitid":"8"} 形
        assert_eq!(graph_data_json(Some("8"), None), r#"{"circuitid":"8"}"#);
    }

    #[test]
    fn graph_data_json_with_date() {
        let json = graph_data_json(None, Some((2023, 9, 10)));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["day"], serde_json::json!([2023, 9, 10]));
        assert_eq!(v["term"], "2023/09/10");
        assert_eq!(v["termStr"], "day");
    }

    #[test]
    fn graph_path_plain_without_params() {
        assert_eq!(graph_path(52111, None, None), "/page/graph/52111");
    }

    #[test]
    fn graph_path_encodes_data() {
        let path = graph_path(584, Some("8"), None);
        // base64({"circuitid":"8"})
        assert_eq!(path, "/page/graph/584?data=eyJjaXJjdWl0aWQiOiI4In0=");
    }

    #[test]
    fn val_kwh_missing_is_parse_failed() {
        let err = parse_val_kwh("<html><body></body></html>").unwrap_err();
        assert_eq!(err.kind, ErrorKind::ParseFailed);
    }

    #[test]
    fn val_kwh_non_numeric_is_parse_failed() {
        let err = parse_val_kwh(r#"<span id="val_kwh">--</span>"#).unwrap_err();
        assert_eq!(err.kind, ErrorKind::ParseFailed);
    }
}
