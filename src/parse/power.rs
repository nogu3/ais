//! `POST /data/electricflow/111/update` の JSON レスポンスを解釈する。
//!
//! 契約（ファーム凍結前提）:
//! - `g_capacity`: 総発電電力 [kW]（数値 or 数値文字列）
//! - `u_capacity`: 総使用電力 [kW]
//! - `lo_buy_sell`: 1 = 売電中、それ以外 = 買電中
//! - `g_d_<N>_title` / `g_d_<N>_capacity`: 発電ソース名と瞬時値 [W]

use serde::Serialize;

use crate::error::{AisError, Result};
use crate::parse::lenient_json_number;

#[derive(Debug, Serialize, PartialEq)]
pub struct Power {
    /// 総発電電力 [kW]（太陽光等の合算）
    pub generation_kw: f64,
    /// 総使用電力 [kW]
    pub usage_kw: f64,
    /// 買電電力 [kW]。売電中は 0.0（売電値の出力は保留事項のため出さない）
    pub buy_kw: f64,
    /// 系統との向き: "buy" | "sell"
    pub grid_direction: String,
    /// 発電ソース内訳（存在するもののみ）
    pub sources: Vec<PowerSource>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct PowerSource {
    pub name: String,
    pub power_w: i64,
}

pub fn parse_power(body: &str) -> Result<Power> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| AisError::parse_failed(format!("electricflow/111/update is not JSON: {e}")))?;

    let usage_kw = lenient_json_number(v.get("u_capacity").unwrap_or(&serde_json::Value::Null))
        .ok_or_else(|| {
            AisError::parse_failed("u_capacity missing or not numeric (firmware mismatch?)")
        })?;

    // 発電なし（太陽光未設置）の家ではキー自体が無い場合があるため 0 扱い
    let generation_kw = v
        .get("g_capacity")
        .and_then(lenient_json_number)
        .unwrap_or(0.0);

    let selling = v
        .get("lo_buy_sell")
        .and_then(lenient_json_number)
        .map(|n| n == 1.0)
        .unwrap_or(false);

    let net_kw = ((generation_kw - usage_kw).abs() * 1000.0).round() / 1000.0;
    let (grid_direction, buy_kw) = if selling {
        ("sell".to_string(), 0.0)
    } else {
        ("buy".to_string(), net_kw)
    };

    let mut sources = Vec::new();
    for i in 1.. {
        let title = match v.get(format!("g_d_{i}_title")).and_then(|t| t.as_str()) {
            Some(t) => t.trim().to_string(),
            None => break,
        };
        let capacity = v
            .get(format!("g_d_{i}_capacity"))
            .and_then(lenient_json_number);
        match capacity {
            Some(w) if !title.is_empty() && title != "-" => sources.push(PowerSource {
                name: title,
                power_w: w.round() as i64,
            }),
            _ => {}
        }
    }

    Ok(Power {
        generation_kw,
        usage_kw,
        buy_kw,
        grid_direction,
        sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    #[test]
    fn rejects_missing_usage() {
        let err = parse_power("{}").unwrap_err();
        assert_eq!(err.kind, ErrorKind::ParseFailed);
    }

    #[test]
    fn rejects_non_json() {
        let err = parse_power("<html></html>").unwrap_err();
        assert_eq!(err.kind, ErrorKind::ParseFailed);
    }

    #[test]
    fn tolerates_missing_generation() {
        let p = parse_power(r#"{"u_capacity":"0.8"}"#).unwrap();
        assert_eq!(p.generation_kw, 0.0);
        assert_eq!(p.usage_kw, 0.8);
        assert_eq!(p.grid_direction, "buy");
        assert_eq!(p.buy_kw, 0.8);
    }
}
