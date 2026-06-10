//! `ais fetch <page>` 用の汎用解釈。任意ページの HTML から
//! `id` 属性付き要素のテキストを正規化して取り出す（生 HTML は流さない）。

use std::collections::BTreeMap;

use scraper::{ElementRef, Html, Selector};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct PageValues {
    pub path: String,
    /// id 属性 → テキスト（空テキストの要素は含めない）
    pub values: BTreeMap<String, String>,
}

pub fn extract_page_values(path: &str, html: &str) -> PageValues {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("[id]").unwrap();

    let mut values = BTreeMap::new();
    for el in doc.select(&sel) {
        let Some(id) = el.value().attr("id") else {
            continue;
        };
        let text = own_text(&el);
        if !text.is_empty() {
            values.insert(id.to_string(), text);
        }
    }

    PageValues {
        path: path.to_string(),
        values,
    }
}

/// 要素直下のテキストのみ（子要素のテキストは含めない）。
/// ネストした id 要素同士でテキストが重複するのを避ける。
fn own_text(el: &ElementRef) -> String {
    el.children()
        .filter_map(|node| node.value().as_text().map(|t| t.to_string()))
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}
