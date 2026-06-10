//! `ais` — Panasonic AiSEG2 専用 CLI のコアライブラリ。
//!
//! レイヤ構成:
//! - [`fetch`] — フェッチ層。HTTP + Digest 認証のみを扱い、HTML/JSON の中身は知らない。
//! - [`parse`] — 解釈層。AiSEG2 のページ構造（ファーム依存の壊れやすい契約）をここに閉じ込める。
//! - [`control`] — 解釈層の一部。制御 AJAX のペイロード生成とレスポンス解釈。

pub mod control;
pub mod error;
pub mod fetch;
pub mod parse;
