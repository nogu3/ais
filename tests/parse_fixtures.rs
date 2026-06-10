//! サニタイズ済みフィクスチャに対する解釈層の統合テスト。
//! CI に実機は不要。セレクタがずれた場合はここで検知する。

use ais::parse::{circuits, devices, energy, generic, power};

#[test]
fn power_buying() {
    let body = include_str!("fixtures/power_update_buy.json");
    let p = power::parse_power(body).unwrap();

    assert_eq!(p.generation_kw, 0.5);
    assert_eq!(p.usage_kw, 1.2);
    assert_eq!(p.grid_direction, "buy");
    assert_eq!(p.buy_kw, 0.7);
    assert_eq!(p.sources.len(), 1);
    assert_eq!(p.sources[0].name, "太陽光");
    assert_eq!(p.sources[0].power_w, 512);
}

#[test]
fn power_selling_hides_sell_value() {
    let body = include_str!("fixtures/power_update_sell.json");
    let p = power::parse_power(body).unwrap();

    assert_eq!(p.generation_kw, 2.4);
    assert_eq!(p.usage_kw, 0.6);
    assert_eq!(p.grid_direction, "sell");
    // 売電値の出力は保留事項のため buy_kw は 0
    assert_eq!(p.buy_kw, 0.0);
}

#[test]
fn circuits_full_page() {
    let html = include_str!("fixtures/electricflow_1113_page1.html");
    let entries = circuits::parse_circuit_page(html);

    assert_eq!(entries.len(), 10);
    assert_eq!(entries[0].name, "リビング エアコン");
    assert_eq!(entries[0].power_w, Some(650));
    assert_eq!(entries[9].name, "トイレ");
    assert_eq!(entries[9].power_w, Some(8));
    assert!(entries.iter().all(|c| c.power_w.unwrap_or(0) > 0));
}

#[test]
fn circuits_last_page_with_zero_and_unmeasured() {
    let html = include_str!("fixtures/electricflow_1113_page2.html");
    let entries = circuits::parse_circuit_page(html);

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].power_w, Some(5));
    assert_eq!(entries[1].name, "玄関 コンセント");
    assert_eq!(entries[1].power_w, Some(0));
    // "-"（計測なし）は null
    assert_eq!(entries[2].power_w, None);
}

#[test]
fn circuits_selector_mismatch_returns_empty() {
    let entries = circuits::parse_circuit_page("<html><body><p>error</p></body></html>");
    assert!(entries.is_empty());
}

#[test]
fn devices_menu_link() {
    let html = include_str!("fixtures/index.html");
    assert_eq!(
        devices::parse_devices_menu_link(html).as_deref(),
        Some("/page/devices/device")
    );
}

#[test]
fn devices_menu_link_missing() {
    assert_eq!(devices::parse_devices_menu_link("<html></html>"), None);
}

#[test]
fn device_panels_with_individual_link() {
    let html = include_str!("fixtures/devices_top.html");
    let panels = devices::parse_panels(html);

    // 個別リンクを持つパネルのみ（エコキュートは一括のみなので含まれない）
    assert_eq!(panels.len(), 2);
    assert_eq!(panels[0].kind, "照明");
    assert_eq!(panels[0].link, "32i1?page=1");
    assert_eq!(panels[1].kind, "空気清浄機");
    assert_eq!(panels[1].link, "32f?page=1");

    assert_eq!(devices::panel_page_id(&panels[0].link), "32i1");
}

#[test]
fn lighting_device_page() {
    let html = include_str!("fixtures/devices_lighting_page1.html");

    assert_eq!(devices::parse_token(html).as_deref(), Some("123456"));

    let found = devices::parse_device_panels(html);
    assert_eq!(found.len(), 3);

    assert_eq!(found[0].id, "1073741825:0x029101");
    assert_eq!(found[0].name, "リビング照明");
    assert_eq!(found[0].state, "on");
    assert_eq!(found[0].dev_type, "0x92");
    assert_eq!(found[0].state_attr.as_deref(), Some("0x30"));

    assert_eq!(found[1].name, "ダイニング照明");
    assert_eq!(found[1].state, "off");
}

#[test]
fn airpurifier_device_page_generic_classes() {
    let html = include_str!("fixtures/devices_airpurifier_page1.html");

    let found = devices::parse_device_panels(html);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "エアイー");
    // kiki_state の子に on クラスが無いので off
    assert_eq!(found[0].state, "off");
    assert_eq!(found[0].dev_type, "0x35");
}

#[test]
fn token_fallback_to_setting_value() {
    let html = r#"<div id="main"><div class="setting_value">98765</div></div>"#;
    assert_eq!(devices::parse_token(html).as_deref(), Some("98765"));
}

#[test]
fn token_fallback_to_control_attr() {
    let html = r#"<div id="main"><div class="control" token="55555"></div></div>"#;
    assert_eq!(devices::parse_token(html).as_deref(), Some("55555"));
}

#[test]
fn energy_graph_val_kwh() {
    let html = include_str!("fixtures/graph_val_kwh.html");
    assert_eq!(energy::parse_val_kwh(html).unwrap(), 12.3);
}

#[test]
fn energy_circuit_catalog() {
    let html = include_str!("fixtures/circuit_catalog_734.html");
    let catalog = energy::parse_circuit_catalog(html).unwrap();

    // strBtnType == "1" のみ（"使用しない" は除外）、名前なしはフォールバック
    assert_eq!(catalog.len(), 3);
    assert_eq!(catalog[0].id, "30");
    assert_eq!(catalog[0].name, "リビング エアコン");
    assert_eq!(catalog[1].name, "IH クッキングヒーター");
    assert_eq!(catalog[2].name, "Circuit 32");
}

#[test]
fn energy_catalog_selector_mismatch() {
    let err = energy::parse_circuit_catalog("<html><body></body></html>").unwrap_err();
    assert_eq!(err.kind, ais::error::ErrorKind::ParseFailed);
}

#[test]
fn fetch_extracts_id_values() {
    let html = r#"<html><body>
        <span id="val_kwh">12.3</span>
        <div id="empty"></div>
        <div id="nested">outer<span id="inner">inner</span></div>
    </body></html>"#;
    let page = generic::extract_page_values("/page/graph/51111", html);

    assert_eq!(page.path, "/page/graph/51111");
    assert_eq!(page.values.get("val_kwh").map(String::as_str), Some("12.3"));
    assert_eq!(page.values.get("inner").map(String::as_str), Some("inner"));
    // 直下テキストのみなので nested は "outer"
    assert_eq!(page.values.get("nested").map(String::as_str), Some("outer"));
    assert!(!page.values.contains_key("empty"));
}
