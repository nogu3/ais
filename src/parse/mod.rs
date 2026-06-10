//! 解釈層。AiSEG2 のページ構造（HTML / 制御 AJAX の JSON）をパースする。
//! ファーム依存の壊れやすい契約はすべてこのモジュール配下に閉じ込める。
//! セレクタ不一致は `parse_failed`（exit 6）として呼び出し側に伝える。

pub mod circuits;
pub mod devices;
pub mod generic;
pub mod power;

/// "650W" / "1,234" / "0.5kW" のような表示文字列から数値部分を取り出す。
/// "-"（計測なし）や空文字は `None`。
pub(crate) fn lenient_number(text: &str) -> Option<f64> {
    let cleaned: String = text
        .trim()
        .replace(['，', ','], "")
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    if cleaned.is_empty() || cleaned == "-" {
        return None;
    }
    cleaned.parse().ok()
}

/// serde_json::Value のフィールドを数値として寛容に読む（数値 / 数値文字列の両対応）。
pub(crate) fn lenient_json_number(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => lenient_number(s),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenient_number_handles_units_and_separators() {
        assert_eq!(lenient_number("650W"), Some(650.0));
        assert_eq!(lenient_number("1,234W"), Some(1234.0));
        assert_eq!(lenient_number("0.5"), Some(0.5));
        assert_eq!(lenient_number("-"), None);
        assert_eq!(lenient_number(""), None);
        assert_eq!(lenient_number("  1.2kW "), Some(1.2));
    }
}
