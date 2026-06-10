//! `GET /page/electricflow/1113?id=<N>&request_by_form=1` の HTML を解釈する。
//!
//! 契約（ファーム凍結前提）:
//! - 回路は消費電力の大きい順に並ぶ
//! - `div.c_device` = 回路名、`div.c_value` = 瞬時値（例 "650W"、計測なしは "-"）
//! - ページングは `id=1..`。値 0 以降は実質終端

use scraper::{Html, Selector};
use serde::Serialize;

use crate::parse::lenient_number;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Circuit {
    pub name: String,
    /// 瞬時電力 [W]。計測なし（"-"）は null
    pub power_w: Option<i64>,
}

/// 1 ページ分の回路エントリを返す。該当要素なしは空 Vec（終端 or セレクタ不一致は呼び出し側で判定）。
pub fn parse_circuit_page(html: &str) -> Vec<Circuit> {
    let doc = Html::parse_document(html);
    let device_sel = Selector::parse("div.c_device").unwrap();
    let value_sel = Selector::parse("div.c_value").unwrap();

    let names = doc.select(&device_sel).map(|el| element_text(&el));
    let values = doc.select(&value_sel).map(|el| {
        let text = element_text(&el);
        lenient_number(&text).map(|w| w.round() as i64)
    });

    names
        .zip(values)
        .map(|(name, power_w)| Circuit { name, power_w })
        .collect()
}

fn element_text(el: &scraper::ElementRef) -> String {
    el.text().collect::<Vec<_>>().join("").trim().to_string()
}
