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
    /// 買電電力 [kW]。売電中は 0.0
    pub buy_kw: f64,
    /// 売電電力 [kW]。買電中は 0.0
    ///
    /// AiSEG2 は売電電力そのものを返さないので `|発電 - 使用|` で求める。
    /// 系統は同時に両方向へ流れないため、`buy_kw` と `sell_kw` の
    /// どちらか一方は必ず 0.0 になる。
    pub sell_kw: f64,
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

    // 系統を流れる電力の大きさ。AiSEG2 は向き(lo_buy_sell)しか返さないので
    // 大きさは自分で求める。向きに応じて buy / sell のどちらか一方に載せる
    // (同時に両方向へは流れないので、他方は 0.0)。
    let net_kw = ((generation_kw - usage_kw).abs() * 1000.0).round() / 1000.0;
    let (grid_direction, buy_kw, sell_kw) = if selling {
        ("sell".to_string(), 0.0, net_kw)
    } else {
        ("buy".to_string(), net_kw, 0.0)
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
        sell_kw,
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
        assert_eq!(p.sell_kw, 0.0);
    }

    #[test]
    fn reports_sell_kw_when_selling() {
        // AiSEG2 は売電電力そのものを返さない。売電中に系統へ流れる電力は
        // |発電 - 使用| で、その値を sell_kw に載せる。
        let p = parse_power(r#"{"g_capacity":"3.4","u_capacity":"1.2","lo_buy_sell":1}"#).unwrap();
        assert_eq!(p.grid_direction, "sell");
        assert_eq!(p.sell_kw, 2.2);
        assert_eq!(p.buy_kw, 0.0, "売電中の買電は 0");
    }

    #[test]
    fn sell_kw_is_zero_when_buying() {
        let p = parse_power(r#"{"g_capacity":"1.4","u_capacity":"2.0","lo_buy_sell":0}"#).unwrap();
        assert_eq!(p.grid_direction, "buy");
        assert_eq!(p.buy_kw, 0.6);
        assert_eq!(p.sell_kw, 0.0, "買電中の売電は 0");
    }

    #[test]
    fn buy_and_sell_are_never_both_nonzero() {
        // 系統は同時に両方向へ流れない。どちらか一方は必ず 0。
        for body in [
            r#"{"g_capacity":"3.4","u_capacity":"1.2","lo_buy_sell":1}"#,
            r#"{"g_capacity":"1.4","u_capacity":"2.0","lo_buy_sell":0}"#,
            r#"{"u_capacity":"0.8"}"#,
        ] {
            let p = parse_power(body).unwrap();
            assert!(
                p.buy_kw == 0.0 || p.sell_kw == 0.0,
                "buy_kw={} sell_kw={} が同時に非ゼロ: {body}",
                p.buy_kw,
                p.sell_kw
            );
        }
    }
}
