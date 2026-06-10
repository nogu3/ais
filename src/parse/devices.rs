//! 機器コントロール関連ページの解釈。
//!
//! 契約（ファーム凍結前提）:
//! - トップページ `#fmenu` 内の `/page/devices` リンクが機器コントロール一覧の入口
//! - 一覧ページの `.panel` ごとに `.kiki_title`（種別名）と「個別」リンク（例 `32i1?page=1`）
//! - 機器ページの `.panel[nodeid]` が 1 機器。`nodeid` / `eoj` / `type` / `state` 属性を持ち、
//!   `*_title` クラスが機器名、`*_state` クラス（自身 or 子に `on`）が状態
//! - 制御 token は `#main[token]`、`.setting_value` テキスト、`.control[token]` のいずれか

use scraper::{ElementRef, Html, Selector};
use serde::Serialize;

/// 機器コントロール一覧の 1 機器。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Device {
    /// `<nodeId>:<eoj>`。on/off コマンドの指定子として一意
    pub id: String,
    pub name: String,
    /// パネル種別名（照明 / 空気清浄機 など。AiSEG2 の表示をそのまま使う）
    pub kind: String,
    /// "on" | "off"
    pub state: String,
    pub node_id: String,
    pub eoj: String,
    #[serde(rename = "type")]
    pub dev_type: String,
    /// 制御エンドポイントのページ ID（例 "32i1"）
    pub link: String,
    /// 制御 token（出力には含めない）
    #[serde(skip)]
    pub token: String,
    /// パネルの state 属性（例 "0x30"）。トグル系制御で使う
    #[serde(skip)]
    pub state_attr: Option<String>,
}

/// 機器種別パネル（一覧ページの 1 行）。
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub kind: String,
    /// 「個別」リンクの href（例 "32i1?page=1"）
    pub link: String,
}

/// トップページから機器コントロール一覧へのリンクを探す。
pub fn parse_devices_menu_link(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse(r#"#fmenu a[href^="/page/devices"]"#).unwrap();
    doc.select(&sel)
        .next()
        .and_then(|a| a.value().attr("href"))
        .map(str::to_string)
}

/// 機器コントロール一覧ページから種別パネル（個別リンク持ちのみ）を抽出する。
pub fn parse_panels(html: &str) -> Vec<Panel> {
    let doc = Html::parse_document(html);
    let panel_sel = Selector::parse("div.panel").unwrap();
    let title_sel = Selector::parse(".kiki_title").unwrap();
    let a_sel = Selector::parse("a").unwrap();

    let mut panels = Vec::new();
    for panel in doc.select(&panel_sel) {
        let kind = match panel.select(&title_sel).next() {
            Some(t) => element_text(&t),
            None => continue,
        };
        // 「個別」ラベルの直後の <a> が個別制御ページへのリンク
        let link = panel.select(&a_sel).find_map(|a| {
            let label = prev_element(&a).map(|el| element_text(&el))?;
            if label == "個別" {
                a.value().attr("href").map(str::to_string)
            } else {
                None
            }
        });
        if let Some(link) = link {
            if !kind.is_empty() {
                panels.push(Panel { kind, link });
            }
        }
    }
    panels
}

/// 機器ページから制御 token を取り出す（ファーム差分を考慮して複数箇所を試す）。
pub fn parse_token(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);

    let main_sel = Selector::parse("#main").unwrap();
    if let Some(token) = doc
        .select(&main_sel)
        .next()
        .and_then(|el| el.value().attr("token"))
        .filter(|t| !t.is_empty())
    {
        return Some(token.to_string());
    }

    let setting_sel = Selector::parse(".setting_value").unwrap();
    if let Some(token) = doc
        .select(&setting_sel)
        .next()
        .map(|el| element_text(&el))
        .filter(|t| !t.is_empty())
    {
        return Some(token);
    }

    let control_sel = Selector::parse(".control[token]").unwrap();
    doc.select(&control_sel)
        .next()
        .and_then(|el| el.value().attr("token"))
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// 機器ページの 1 ページ分から機器を抽出する。`kind` / `link` / `token` は呼び出し側で補完する。
pub fn parse_device_panels(html: &str) -> Vec<Device> {
    let doc = Html::parse_document(html);
    let panel_sel = Selector::parse("div.panel[nodeid]").unwrap();

    let mut devices = Vec::new();
    for panel in doc.select(&panel_sel) {
        let node_id = panel.value().attr("nodeid").unwrap_or_default().to_string();
        let eoj = panel.value().attr("eoj").unwrap_or_default().to_string();
        let dev_type = panel.value().attr("type").unwrap_or_default().to_string();
        let state_attr = panel.value().attr("state").map(str::to_string);

        let name = find_class_suffix_text(&panel, "_title").unwrap_or_default();
        let is_on = find_class_suffix_element(&panel, "_state")
            .map(|el| has_on_class(&el))
            .unwrap_or(false);

        devices.push(Device {
            id: format!("{node_id}:{eoj}"),
            name,
            kind: String::new(),
            state: if is_on { "on" } else { "off" }.to_string(),
            node_id,
            eoj,
            dev_type,
            link: String::new(),
            token: String::new(),
            state_attr,
        });
    }
    devices
}

/// パネル種別リンク（"32i1?page=1"）から制御パス用のページ ID（"32i1"）を取り出す。
pub fn panel_page_id(link: &str) -> &str {
    link.split('?').next().unwrap_or(link)
}

fn element_text(el: &ElementRef) -> String {
    el.text().collect::<Vec<_>>().join("").trim().to_string()
}

fn prev_element<'a>(el: &ElementRef<'a>) -> Option<ElementRef<'a>> {
    el.prev_siblings().find_map(ElementRef::wrap)
}

/// 子孫から、クラス名のいずれかが `suffix` で終わる最初の要素を探す。
fn find_class_suffix_element<'a>(root: &ElementRef<'a>, suffix: &str) -> Option<ElementRef<'a>> {
    root.descendants().find_map(|node| {
        let el = ElementRef::wrap(node)?;
        if el.value().classes().any(|c| c.ends_with(suffix)) {
            Some(el)
        } else {
            None
        }
    })
}

fn find_class_suffix_text(root: &ElementRef, suffix: &str) -> Option<String> {
    find_class_suffix_element(root, suffix)
        .map(|el| element_text(&el))
        .filter(|t| !t.is_empty())
}

/// 状態要素自身、または直下の子要素に `on` クラスがあるか。
fn has_on_class(el: &ElementRef) -> bool {
    if el.value().classes().any(|c| c == "on") {
        return true;
    }
    el.children()
        .filter_map(ElementRef::wrap)
        .any(|child| child.value().classes().any(|c| c == "on"))
}
