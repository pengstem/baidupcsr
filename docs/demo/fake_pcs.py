#!/usr/bin/env python3
"""A fake Baidu Netdisk for recording the README demos.

It runs as an HTTP proxy: point `pcs_config.json`'s `proxy` at it and leave
`enable_https` off, and every `pcs.baidu.com` / download-host request lands
here instead of on Baidu. HTTPS (`CONNECT`, only used for the quota) is
refused, so nothing leaves the machine.

    python3 fake_pcs.py --port 18765 --assets ./assets --repo ../..
"""

import argparse
import json
import re
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, quote, urlsplit

GiB = 1024**3
MiB = 1024**2
KiB = 1024

# Per-request transfer rate; the client keeps 8 ranges in flight.
RANGE_BYTES_PER_SEC = 4 * MiB


class Node:
    def __init__(self, path, is_dir, size=0, mtime=0, content=None):
        self.path = path
        self.is_dir = is_dir
        self.size = size
        self.mtime = mtime
        self.content = content  # bytes, or None for synthetic data

    def entry(self, fs_id):
        return {
            "fs_id": fs_id,
            "path": self.path,
            "server_filename": self.path.rsplit("/", 1)[-1],
            "mtime": self.mtime,
            "size": self.size,
            "isdir": int(self.is_dir),
        }


class Tree:
    def __init__(self):
        self.lock = threading.Lock()
        self.nodes = {"/": Node("/", True)}
        self.ids = {}
        self.clock = 1_790_000_000  # 2026-09

    def _id(self, path):
        return self.ids.setdefault(path, 900_000_000_000 + len(self.ids) * 7919)

    def add(self, path, size=0, content=None, days_ago=0.0, is_dir=False):
        parts = path.strip("/").split("/")
        for depth in range(1, len(parts)):
            parent = "/" + "/".join(parts[:depth])
            self.nodes.setdefault(parent, Node(parent, True, mtime=self.clock))
        if content is not None:
            size = len(content)
        mtime = int(self.clock + 30 * 86400 - days_ago * 86400)
        self.nodes[path] = Node(path, is_dir, size, mtime, content)

    def children(self, path):
        prefix = path.rstrip("/") + "/"
        return [
            node
            for key, node in self.nodes.items()
            if key != "/" and key.startswith(prefix) and "/" not in key[len(prefix) :]
        ]

    def list(self, path):
        with self.lock:
            node = self.nodes.get(path)
            if node is None or not node.is_dir:
                return None
            kids = sorted(self.children(path), key=lambda n: n.path)
            return [n.entry(self._id(n.path)) for n in kids]

    def search(self, root, keyword):
        prefix = root.rstrip("/") + "/"
        with self.lock:
            hits = [
                n
                for key, n in self.nodes.items()
                if key.startswith(prefix) and keyword.lower() in key.rsplit("/", 1)[-1].lower()
            ]
            return [n.entry(self._id(n.path)) for n in sorted(hits, key=lambda n: n.path)]

    def subtree(self, path):
        prefix = path.rstrip("/") + "/"
        return [key for key in self.nodes if key == path or key.startswith(prefix)]

    def remove(self, path):
        for key in self.subtree(path):
            del self.nodes[key]

    def transfer(self, source, target, keep):
        moved = {}
        for key in self.subtree(source):
            node = self.nodes[key]
            new = target + key[len(source) :]
            moved[new] = Node(new, node.is_dir, node.size, int(time.time()), node.content)
        if not keep:
            self.remove(source)
        self.nodes.update(moved)


def build_tree(assets: Path, repo: Path) -> Tree:
    tree = Tree()
    add = tree.add

    movies = [
        ("Big.Buck.Bunny.2008.2160p.60fps.mp4", 0.83 * GiB, 120),
        ("Cosmos.Laundromat.2015.1080p.mkv", 1.52 * GiB, 80),
        ("Elephants.Dream.2006.1080p.mkv", 0.68 * GiB, 200),
        ("Sintel.2010.4K.UHD.HDR.mkv", 4.71 * GiB, 60),
        ("Spring.2019.2160p.WEB-DL.mkv", 2.23 * GiB, 30),
        ("Tears.of.Steel.2012.2160p.mkv", 6.05 * GiB, 9),
    ]
    for name, size, days in movies:
        add(f"/我的资源/开源电影/{name}", int(size), days_ago=days)
    add("/我的资源/开源电影/字幕/Sintel.zh-CN.ass", 48_213, days_ago=60)
    add("/我的资源/开源电影/字幕/Tears.of.Steel.zh-CN.srt", 31_970, days_ago=9)
    for episode in range(1, 13):
        add(
            f"/我的资源/纪录片/宇宙的奇迹 S01/宇宙的奇迹.S01E{episode:02}.1080p.mp4",
            int((1.1 + episode * 0.07) * GiB),
            days_ago=40 - episode,
        )
    add("/我的资源/纪录片/宇宙的奇迹 S01/poster.jpg", content=(assets / "galaxy.jpg").read_bytes(), days_ago=40)

    for image, days in [
        ("曼德博集合.jpg", 3),
        ("极光.jpg", 12),
        ("星云.jpg", 20),
        ("等离子.jpg", 33),
        ("渐变·黄昏.jpg", 41),
    ]:
        add(f"/壁纸/{image}", content=(assets / image).read_bytes(), days_ago=days)

    notes = (Path(__file__).parent / "notes").glob("*.md")
    for note in sorted(notes):
        add(f"/文档/{note.name}", content=note.read_bytes(), days_ago=2)
    add("/文档/课程设计/需求报告.pdf", 1_482_113, days_ago=5)
    add("/文档/课程设计/设计说明书.docx", 2_093_555, days_ago=4)
    add("/文档/课程设计/答辩.pptx", 18_449_027, days_ago=1)
    add("/文档/发票/2026-09 云服务器.pdf", 88_412, days_ago=20)
    add("/文档/发票/2026-10 域名续费.pdf", 61_027, days_ago=6)

    for source in ["Cargo.toml", "README.md", "src/main.rs", "src/cli.rs", "src/lib.rs",
                   "src/pcs/client.rs", "src/app/download.rs", "src/app/fetch.rs",
                   "src/tui/keymap.rs", "src/tui/theme.rs"]:
        path = repo / source
        if path.exists():
            add(f"/代码/baidupcsr/{source}", content=path.read_bytes(), days_ago=1)
    add("/代码/dotfiles/.zshrc", content=b"# zsh\nexport EDITOR=nvim\nalias ll='ls -lah'\n", days_ago=90)
    add("/代码/dotfiles/init.lua", content=b"vim.opt.number = true\nvim.opt.relativenumber = true\n", days_ago=90)

    for index, (title, size) in enumerate(
        [("Morning Coffee", 31.2), ("Rainy Window", 28.7), ("Late Night Train", 35.9),
         ("Paper Lanterns", 26.4), ("Slow Tide", 41.0)], start=1
    ):
        add(f"/音乐/Lo-fi 合集/{index:02} {title}.flac", int(size * MiB), days_ago=70)

    add("/软件/archlinux-2026.10.01-x86_64.iso", int(1.31 * GiB), days_ago=11)
    add("/软件/mpv-x86_64-v3-20261001.7z", int(42.6 * MiB), days_ago=11)
    add("/软件/ffmpeg-7.2-full_build.zip", int(201.4 * MiB), days_ago=15)
    add("/软件/JetBrainsMono-2.304.zip", int(5.4 * MiB), days_ago=150)
    add("/软件/SHA256SUMS", content=b"9f3c...  archlinux-2026.10.01-x86_64.iso\n", days_ago=11)

    add("/备份/手机相册-2026-H1.zip", int(23.8 * GiB), days_ago=100)
    add("/备份/笔记本-home-20261001.tar.zst", int(61.2 * GiB), days_ago=11)
    add("/备份/数据库/blog-20261010.sql.gz", int(12.1 * MiB), days_ago=2)
    return tree


def synthetic(start, length):
    pattern = b"baidupcsr-demo-data\n"
    offset = start % len(pattern)
    repeated = pattern * (length // len(pattern) + 2)
    return repeated[offset : offset + length]


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    tree: Tree = None  # set in main
    assets: Path = None

    def log_message(self, *_):
        pass

    def do_CONNECT(self):  # HTTPS (quota): refuse, never tunnel out
        self.send_response(403)
        self.send_header("Content-Length", "0")
        self.send_header("Connection", "close")
        self.end_headers()

    def do_GET(self):
        self.route()

    def do_POST(self):
        self.route()

    def reply(self, status, body, content_type="application/json", headers=()):
        if not isinstance(body, bytes):
            body = json.dumps(body, ensure_ascii=False).encode()
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        for name, value in headers:
            self.send_header(name, value)
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass  # the client cancelled the range

    def body(self):
        length = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(length) if length else b""

    def route(self):
        url = urlsplit(self.path)
        query = {key: values[0] for key, values in parse_qs(url.query).items()}
        payload = self.body()
        if url.path.startswith("/rest/2.0/pcs/thumbnail"):
            return self.thumbnail(query)
        if url.path.startswith("/rest/2.0/pcs/file"):
            return self.file_api(query, payload)
        if url.path.startswith("/file/"):
            return self.download(query)
        self.reply(404, {"error_code": 31066, "error_msg": "not found"})

    def file_api(self, query, payload):
        method = query.get("method")
        tree = self.tree
        if method == "list":
            entries = tree.list(query.get("path", "/"))
            if entries is None:
                return self.reply(200, {"error_code": 31066, "error_msg": "file does not exist"})
            time.sleep(0.12)
            return self.reply(200, {"list": entries})
        if method == "search":
            return self.reply(200, {"list": tree.search(query.get("path", "/"), query.get("wd", ""))})
        if method == "locatedownload":
            path = query.get("path", "")
            fs_id = tree._id(path)
            urls = [
                {"url": f"http://d{n}.pcs.baidu.com/file/{fs_id}?path={quote(path)}", "encrypt": 0}
                for n in (1, 2)
            ]
            return self.reply(200, {"urls": urls})
        if method == "mkdir":
            with tree.lock:
                tree.add(query["path"], is_dir=True)
                tree.nodes[query["path"]].mtime = int(time.time())
            return self.reply(200, {"fs_id": 1, "path": query["path"]})
        if method in ("delete", "copy", "move"):
            match = re.search(rb'name="param"\r\n(?:[^\r\n]+\r\n)*\r\n(.*?)\r\n--', payload, re.S)
            items = json.loads(match.group(1))["list"] if match else []
            time.sleep(0.4)
            with tree.lock:
                for item in items:
                    if method == "delete":
                        tree.remove(item["path"])
                    else:
                        tree.transfer(item["from"], item["to"], keep=method == "copy")
            return self.reply(200, {"extra": {"list": items}})
        self.reply(200, {"error_code": 31023, "error_msg": f"unknown method {method}"})

    def thumbnail(self, query):
        node = self.tree.nodes.get(query.get("path", ""))
        if node is None or node.content is None:
            return self.reply(200, {"error_code": 31066, "error_msg": "file does not exist"})
        time.sleep(0.25)
        self.reply(200, node.content, "image/jpeg")

    def download(self, query):
        node = self.tree.nodes.get(query.get("path", ""))
        match = re.fullmatch(r"bytes=(\d+)-(\d+)", self.headers.get("Range", ""))
        if node is None or node.is_dir or match is None:
            return self.reply(403, b"", "text/plain")
        start, end = int(match.group(1)), min(int(match.group(2)), node.size - 1)
        length = end - start + 1
        if node.content is not None:
            data = node.content[start : end + 1]
        else:
            data = synthetic(start, length)
        time.sleep(length / RANGE_BYTES_PER_SEC)
        self.reply(
            206,
            data,
            "application/octet-stream",
            [("Content-Range", f"bytes {start}-{end}/{node.size}")],
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--port", type=int, default=18765)
    parser.add_argument("--assets", type=Path, default=Path(__file__).parent / "assets")
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    Handler.tree = build_tree(args.assets, args.repo)
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"fake pcs proxy on 127.0.0.1:{args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
