//! 制御 AJAX（`/action/devices/device/<page>/change` ほか）のペイロード生成と
//! レスポンス解釈。解釈層の一部であり、ファーム依存はここに閉じ込める。
//!
//! 契約（ファーム凍結前提）:
//! - ボディは `data=<URL エンコードした JSON>`
//! - 照明（リンクプラス）: `{"token","nodeId","eoj","type","device":{"onoff":"0x30|0x31","modulate":"-"}}`
//!   `onoff` は希望状態（0x30 = ON / 0x31 = OFF）
//! - トグル系（空気清浄機等）: `{"token","nodeId","eoj","type","state":"<現在状態>"}`
//!   現在状態を送ると AiSEG2 側で反転する
//! - レスポンス: `{"result":"0","acceptId":"108946","errorInfo":"-"}`。result != 0 はリジェクト
//! - acceptId があれば `/data/devices/device/<page>/check` に
//!   `{"acceptId":"...","type":"<devType>"}` を投げて完了を確認（0=完了 / 1=実行中 / 2=不明）

use crate::error::{AisError, ErrorKind, Result};
use crate::parse::devices::Device;

pub const ONOFF_ON: &str = "0x30";
pub const ONOFF_OFF: &str = "0x31";

/// 照明（リンクプラス）の on/off ペイロード。`onoff` は希望状態。
pub fn light_change_payload(token: &str, device: &Device, on: bool) -> String {
    serde_json::json!({
        "token": token,
        "nodeId": device.node_id,
        "eoj": device.eoj,
        "type": device.dev_type,
        "device": {
            "onoff": if on { ONOFF_ON } else { ONOFF_OFF },
            "modulate": "-",
        },
    })
    .to_string()
}

/// トグル系機器のペイロード。現在状態を送ると反転する。
pub fn toggle_change_payload(token: &str, device: &Device) -> String {
    let current = device.state_attr.clone().unwrap_or_else(|| {
        if device.state == "on" {
            ONOFF_ON.to_string()
        } else {
            ONOFF_OFF.to_string()
        }
    });
    serde_json::json!({
        "token": token,
        "nodeId": device.node_id,
        "eoj": device.eoj,
        "type": device.dev_type,
        "state": current,
    })
    .to_string()
}

/// `/check` 用ペイロード。
pub fn check_payload(accept_id: &str, dev_type: &str) -> String {
    serde_json::json!({ "acceptId": accept_id, "type": dev_type }).to_string()
}

/// change レスポンスの解釈。成功なら非同期確認用の acceptId（あれば）を返す。
pub fn parse_change_response(body: &str) -> Result<Option<String>> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| AisError::parse_failed(format!("change response is not JSON: {e}")))?;

    let ok = match v.get("result") {
        Some(serde_json::Value::String(s)) => s == "0",
        Some(serde_json::Value::Number(n)) => n.as_i64() == Some(0),
        _ => false,
    };
    if !ok {
        let error_info = v
            .get("errorInfo")
            .and_then(|e| e.as_str())
            .unwrap_or("unknown");
        return Err(AisError::new(
            ErrorKind::ControlRejected,
            format!("AiSEG2 rejected the control request (errorInfo: {error_info})"),
        ));
    }

    let accept_id = match v.get("acceptId") {
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        Some(serde_json::Value::String(s))
            if !s.is_empty() && s != "-" && s.chars().all(|c| c.is_ascii_digit()) =>
        {
            Some(s.clone())
        }
        _ => None,
    };
    Ok(accept_id)
}

#[derive(Debug, PartialEq, Eq)]
pub enum CheckStatus {
    /// 0: 完了
    Done,
    /// 1: 実行中
    InProgress,
    /// 2 等: 失敗 / 不明
    Failed(String),
}

pub fn parse_check_response(body: &str) -> Result<CheckStatus> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| AisError::parse_failed(format!("check response is not JSON: {e}")))?;
    let result = match v.get("result") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => return Err(AisError::parse_failed("check response has no result field")),
    };
    Ok(match result.as_str() {
        "0" => CheckStatus::Done,
        "1" => CheckStatus::InProgress,
        other => CheckStatus::Failed(other.to_string()),
    })
}

/// `data=` ボディ用のパーセントエンコード（RFC 3986 unreserved 以外をエスケープ）。
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_device() -> Device {
        Device {
            id: "1073741825:0x029101".into(),
            name: "リビング照明".into(),
            kind: "照明".into(),
            state: "off".into(),
            node_id: "1073741825".into(),
            eoj: "0x029101".into(),
            dev_type: "0x92".into(),
            link: "32i1".into(),
            token: "123456".into(),
            state_attr: Some("0x31".into()),
        }
    }

    #[test]
    fn light_payload_shape() {
        let payload = light_change_payload("123456", &sample_device(), true);
        let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["token"], "123456");
        assert_eq!(v["nodeId"], "1073741825");
        assert_eq!(v["eoj"], "0x029101");
        assert_eq!(v["type"], "0x92");
        assert_eq!(v["device"]["onoff"], "0x30");
        assert_eq!(v["device"]["modulate"], "-");
    }

    #[test]
    fn toggle_payload_sends_current_state() {
        let payload = toggle_change_payload("123456", &sample_device());
        let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["state"], "0x31");
        assert!(v.get("device").is_none());
    }

    #[test]
    fn change_response_success_with_accept_id() {
        let accept =
            parse_change_response(r#"{"result":"0","acceptId":"108946","errorInfo":"-"}"#).unwrap();
        assert_eq!(accept.as_deref(), Some("108946"));
    }

    #[test]
    fn change_response_success_without_accept_id() {
        let accept =
            parse_change_response(r#"{"result":"0","acceptId":"-","errorInfo":"-"}"#).unwrap();
        assert_eq!(accept, None);
    }

    #[test]
    fn change_response_rejected() {
        let err = parse_change_response(r#"{"result":"1","errorInfo":"busy"}"#).unwrap_err();
        assert_eq!(err.kind, crate::error::ErrorKind::ControlRejected);
        assert!(err.detail.contains("busy"));
    }

    #[test]
    fn check_response_states() {
        assert_eq!(
            parse_check_response(r#"{"result":"0"}"#).unwrap(),
            CheckStatus::Done
        );
        assert_eq!(
            parse_check_response(r#"{"result":"1"}"#).unwrap(),
            CheckStatus::InProgress
        );
        assert_eq!(
            parse_check_response(r#"{"result":"2"}"#).unwrap(),
            CheckStatus::Failed("2".into())
        );
    }

    #[test]
    fn urlencode_escapes_json() {
        assert_eq!(urlencode(r#"{"a":"b c"}"#), "%7B%22a%22%3A%22b%20c%22%7D");
        assert_eq!(urlencode("abc-_.~123"), "abc-_.~123");
    }
}
