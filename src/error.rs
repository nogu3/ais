//! エラー型。`kind` ごとに exit code と stderr 向け構造化 JSON を定義する。

use std::fmt;

/// エラー分類。stderr の `{"error":{"kind":...}}` と exit code に対応する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// ネットワーク到達不能・接続失敗 (exit 3)
    Network,
    /// タイムアウト (exit 3)
    Timeout,
    /// Digest 認証失敗 = 401 (exit 4)
    AuthFailed,
    /// AiSEG2 が想定外の HTTP ステータスを返した (exit 5)
    HttpStatus,
    /// セレクタ不一致・想定外フォーマット = ファームがずれた可能性 (exit 6)
    ParseFailed,
    /// 制御リジェクト / 結果未確認 (exit 7)
    ControlRejected,
    /// 指定機器が機器コントロール一覧に見つからない (exit 11)
    DeviceNotFound,
    /// 指定が複数機器に一致して特定できない (exit 11)
    DeviceAmbiguous,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Network => "network",
            ErrorKind::Timeout => "timeout",
            ErrorKind::AuthFailed => "auth_failed",
            ErrorKind::HttpStatus => "http_status",
            ErrorKind::ParseFailed => "parse_failed",
            ErrorKind::ControlRejected => "control_rejected",
            ErrorKind::DeviceNotFound => "device_not_found",
            ErrorKind::DeviceAmbiguous => "device_ambiguous",
        }
    }

    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::Network | ErrorKind::Timeout => 3,
            ErrorKind::AuthFailed => 4,
            ErrorKind::HttpStatus => 5,
            ErrorKind::ParseFailed => 6,
            ErrorKind::ControlRejected => 7,
            ErrorKind::DeviceNotFound | ErrorKind::DeviceAmbiguous => 11,
        }
    }
}

#[derive(Debug)]
pub struct AisError {
    pub kind: ErrorKind,
    pub detail: String,
}

impl AisError {
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub fn parse_failed(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::ParseFailed, detail)
    }

    /// stderr に流す構造化ログ表現。
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "error": { "kind": self.kind.as_str(), "detail": self.detail }
        })
    }
}

impl fmt::Display for AisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.detail)
    }
}

impl std::error::Error for AisError {}

pub type Result<T> = std::result::Result<T, AisError>;
