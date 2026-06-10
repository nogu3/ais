//! HTTP Digest 認証 (RFC 2617, MD5, qop=auth) の自前実装。
//! AiSEG2 は MD5 + qop=auth のみなので、それ以外（auth-int / SHA-256）は扱わない。

/// `WWW-Authenticate: Digest ...` ヘッダから取り出したチャレンジ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    pub qop_auth: bool,
}

/// `WWW-Authenticate` ヘッダをパースする。Digest 以外は `None`。
pub fn parse_challenge(header: &str) -> Option<Challenge> {
    let rest = header.trim().strip_prefix("Digest")?.trim_start();

    let mut realm = None;
    let mut nonce = None;
    let mut opaque = None;
    let mut qop_auth = false;

    for (key, value) in parse_kv_pairs(rest) {
        match key.as_str() {
            "realm" => realm = Some(value),
            "nonce" => nonce = Some(value),
            "opaque" => opaque = Some(value),
            "qop" => qop_auth = value.split(',').any(|q| q.trim() == "auth"),
            _ => {}
        }
    }

    Some(Challenge {
        realm: realm?,
        nonce: nonce?,
        opaque,
        qop_auth,
    })
}

/// `key="value"` / `key=value` のカンマ区切り列をパースする（引用符内のカンマを尊重）。
fn parse_kv_pairs(s: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut chars = s.chars().peekable();

    loop {
        // key
        let mut key = String::new();
        for c in chars.by_ref() {
            if c == '=' {
                break;
            }
            key.push(c);
        }
        let key = key.trim().to_string();
        if key.is_empty() {
            break;
        }

        // value（引用符付き or 裸）
        let mut value = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                value.push(c);
            }
            // 次のカンマまで読み飛ばす
            for c in chars.by_ref() {
                if c == ',' {
                    break;
                }
            }
        } else {
            let mut ended = false;
            for c in chars.by_ref() {
                if c == ',' {
                    ended = true;
                    break;
                }
                value.push(c);
            }
            value = value.trim().to_string();
            let _ = ended;
        }
        pairs.push((key, value));
    }

    pairs
}

fn md5_hex(input: &str) -> String {
    format!("{:x}", md5::compute(input.as_bytes()))
}

/// `Authorization: Digest ...` ヘッダ値を組み立てる。
/// `cnonce` / `nc` はテスト容易性のため呼び出し側から渡す。
pub fn authorization(
    user: &str,
    pass: &str,
    method: &str,
    uri: &str,
    challenge: &Challenge,
    cnonce: &str,
    nc: &str,
) -> String {
    let ha1 = md5_hex(&format!("{user}:{}:{pass}", challenge.realm));
    let ha2 = md5_hex(&format!("{method}:{uri}"));

    let response = if challenge.qop_auth {
        md5_hex(&format!(
            "{ha1}:{}:{nc}:{cnonce}:auth:{ha2}",
            challenge.nonce
        ))
    } else {
        md5_hex(&format!("{ha1}:{}:{ha2}", challenge.nonce))
    };

    let mut auth = format!(
        "Digest username=\"{user}\", realm=\"{}\", nonce=\"{}\", uri=\"{uri}\", response=\"{response}\"",
        challenge.realm, challenge.nonce
    );
    if challenge.qop_auth {
        auth.push_str(&format!(", qop=auth, nc={nc}, cnonce=\"{cnonce}\""));
    }
    if let Some(opaque) = &challenge.opaque {
        auth.push_str(&format!(", opaque=\"{opaque}\""));
    }
    auth
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 2617 3.5 のテストベクタ
    const RFC_HEADER: &str = "Digest realm=\"testrealm@host.com\", qop=\"auth,auth-int\", nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", opaque=\"5ccc069c403ebaf9f0171e9517f40e41\"";

    #[test]
    fn parses_rfc2617_challenge() {
        let c = parse_challenge(RFC_HEADER).unwrap();
        assert_eq!(c.realm, "testrealm@host.com");
        assert_eq!(c.nonce, "dcd98b7102dd2f0e8b11d0f600bfb0c093");
        assert_eq!(
            c.opaque.as_deref(),
            Some("5ccc069c403ebaf9f0171e9517f40e41")
        );
        assert!(c.qop_auth);
    }

    #[test]
    fn computes_rfc2617_response() {
        let c = parse_challenge(RFC_HEADER).unwrap();
        let auth = authorization(
            "Mufasa",
            "Circle Of Life",
            "GET",
            "/dir/index.html",
            &c,
            "0a4f113b",
            "00000001",
        );
        assert!(
            auth.contains("response=\"6629fae49393a05397450978507c4ef1\""),
            "auth header was: {auth}"
        );
        assert!(auth.contains("opaque=\"5ccc069c403ebaf9f0171e9517f40e41\""));
        assert!(auth.contains("qop=auth"));
    }

    #[test]
    fn rejects_non_digest() {
        assert!(parse_challenge("Basic realm=\"x\"").is_none());
    }

    #[test]
    fn parses_unquoted_values() {
        let c = parse_challenge("Digest realm=\"AiSEG\", nonce=\"abc\", qop=auth").unwrap();
        assert!(c.qop_auth);
        assert_eq!(c.nonce, "abc");
    }
}
