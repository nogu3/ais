#!/usr/bin/env python3
"""Digest 認証付きのモック AiSEG2 サーバ。

`tools/e2e.sh` から使う、実機なしの E2E スモーク用。tests/fixtures/ の
サニタイズ済みフィクスチャをそのまま配信する。クレデンシャルはダミー
（aiseg / secret）で、127.0.0.1:18080 にのみバインドする。
"""

import hashlib
import json
import re
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

USER, PASS, REALM, NONCE, OPAQUE = "aiseg", "secret", "AiSEG", "abcnonce123", "opq456"
FIXTURES = Path(__file__).resolve().parent.parent / "tests" / "fixtures"
PORT = 18080


def md5(s: str) -> str:
    return hashlib.md5(s.encode()).hexdigest()


def check_digest(handler, method: str) -> bool:
    auth = handler.headers.get("Authorization", "")
    if not auth.startswith("Digest "):
        return False
    fields = dict(re.findall(r'(\w+)="?([^",]+)"?', auth))
    ha1 = md5(f"{USER}:{REALM}:{PASS}")
    ha2 = md5(f"{method}:{fields.get('uri', '')}")
    expected = md5(
        f"{ha1}:{NONCE}:{fields.get('nc', '')}:{fields.get('cnonce', '')}:auth:{ha2}"
    )
    return fields.get("response") == expected and fields.get("uri") == handler.path


def fixture(name: str) -> str:
    return (FIXTURES / name).read_text(encoding="utf-8")


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def _challenge(self):
        self.send_response(401)
        self.send_header(
            "WWW-Authenticate",
            f'Digest realm="{REALM}", nonce="{NONCE}", qop="auth", opaque="{OPAQUE}"',
        )
        self.end_headers()

    def _reply(self, body: str, ctype: str = "text/html"):
        data = body.encode()
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _route(self, method: str):
        if not check_digest(self, method):
            return self._challenge()
        p = self.path
        if p == "/data/electricflow/111/update":
            self._reply(fixture("power_update_buy.json"), "application/json")
        elif p.startswith("/page/electricflow/1113?id=1"):
            self._reply(fixture("electricflow_1113_page1.html"))
        elif p.startswith("/page/electricflow/1113?id=2"):
            self._reply(fixture("electricflow_1113_page2.html"))
        elif re.match(r"^/page/graph/5[1-4]111(\?data=.*)?$", p):
            self._reply(fixture("graph_val_kwh.html"))
        elif p.startswith("/page/graph/584?data="):
            self._reply(fixture("graph_val_kwh.html"))
        elif p == "/page/setting/installation/734":
            self._reply(fixture("circuit_catalog_734.html"))
        elif p == "/":
            self._reply(fixture("index.html"))
        elif p == "/page/devices/device":
            self._reply(fixture("devices_top.html"))
        elif p.startswith("/page/devices/device/32i1"):
            self._reply(fixture("devices_lighting_page1.html"))
        elif p.startswith("/page/devices/device/32f"):
            self._reply(fixture("devices_airpurifier_page1.html"))
        elif p == "/action/devices/device/32i1/change":
            self._reply('{"result":"0","acceptId":"108946","errorInfo":"-"}', "application/json")
        elif p == "/data/devices/device/32i1/check":
            self._reply('{"result":"0"}', "application/json")
        else:
            self.send_response(404)
            self.end_headers()

    def do_GET(self):
        self._route("GET")

    def do_POST(self):
        # ボディは読み捨てる（Digest 検証は uri ベース）
        length = int(self.headers.get("Content-Length", 0))
        if length:
            self.rfile.read(length)
        self._route("POST")


if __name__ == "__main__":
    print(f"mock AiSEG2 listening on 127.0.0.1:{PORT}", flush=True)
    HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
