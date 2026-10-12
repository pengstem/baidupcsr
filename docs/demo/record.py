#!/usr/bin/env python3
"""Record README demo GIFs of baidupcsr-cli against fake_pcs.py.

Drives the real binary in a pseudo-terminal with scripted keystrokes, writes
an asciicast v2 file per scene and renders it with agg
(https://github.com/asciinema/agg).

    cargo build --release
    ./make_assets.sh
    python3 record.py                # every scene
    python3 record.py browse search  # some scenes
"""

import codecs
import fcntl
import json
import os
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
BINARY = REPO / "target/release/baidupcsr-cli"
GIF_DIR = REPO / "docs/images"
PORT = 18765
COLS, ROWS = 118, 30

ESC, ENTER = "\x1b", "\r"


def typed(text, pause=0.09):
    """Type `text` one key at a time."""
    return [(char, pause) for char in text]


def go(path_keys):
    return [(key, 0.55) for key in path_keys]


# Root, sorted by name: 代码 壁纸 备份 我的资源 文档 软件 音乐.
SCENES = {
    # Miller columns, text preview, image thumbnails, help.
    "browse": [
        ("", 1.6),
        ("l", 0.8), ("l", 1.0),  # 代码/baidupcsr: src/ Cargo.toml README.md
        ("j", 1.2), ("j", 1.8),  # Cargo.toml, README.md
        ("gg", 0.6), ("l", 1.0),  # src/: app/ pcs/ tui/ cli.rs lib.rs main.rs
        ("G", 0.6), ("k", 0.6), ("k", 2.4),  # cli.rs
        ("h", 0.5), ("h", 0.5), ("h", 0.8),
        ("j", 0.8),  # 壁纸
        ("l", 2.0),
        ("j", 2.0), ("j", 2.0), ("j", 2.0),
        ("h", 0.8),
        ("?", 2.8), (ESC, 1.0),
    ],
    # Mark several files, download them in parallel, watch the task panel.
    "download": [
        ("", 1.4),
        *go("jjjjj"),  # 软件: archlinux ffmpeg JetBrainsMono mpv SHA256SUMS
        ("l", 1.2),
        (" ", 0.5), (" ", 0.5), (" ", 0.8),  # mark the first three
        ("D", 2.5),
        ("T", 3.5),
        ("j", 0.5), ("j", 0.6), ("c", 2.0),  # cancel the oldest (the iso)
        (ESC, 2.5),
    ],
    # Live filter, then recursive and server-side search.
    "search": [
        ("", 1.4),
        *go("jjj"), ("l", 0.8), ("l", 1.0),  # 我的资源/开源电影
        ("/", 0.5), *typed("2160"), ("", 1.0), (ENTER, 1.2),
        (ESC, 0.8),
        ("h", 0.5), ("h", 0.8),
        ("s", 0.6), *typed("mkv"), ("", 0.4), (ENTER, 2.4),
        ("j", 0.5), ("j", 0.5), ("j", 1.0),
        (ESC, 1.0),
        (":", 0.5), *typed("search --api 宇宙"), ("", 0.4), (ENTER, 2.4),
        (ESC, 1.0),
    ],
    # mkdir, yank & paste, rename, delete with confirmation.
    "fileops": [
        ("", 1.4),
        *go("jjjj"), ("l", 1.0),  # 文档: 发票/ 课程设计/ Linux 命令速查.md 读书笔记…
        ("a", 0.5), *typed("归档"), ("", 0.5), (ENTER, 1.8),
        ("G", 0.6), ("k", 0.5),
        (" ", 0.5), (" ", 0.8),  # mark both notes
        ("y", 0.8),
        ("gg", 0.5), ("j", 0.5), ("l", 1.0),  # into 归档
        ("p", 2.2),
        ("r", 0.8), ("\x15", 0.3), *typed("命令速查.md"), ("", 0.4), (ENTER, 2.0),
        ("j", 0.5), ("dd", 1.6), ("y", 2.0),
        ("h", 1.4),
    ],
}


def pcs_config(home: Path):
    config = {
        "baidu_active_uid": 2026,
        "baidu_user_list": [
            {"uid": 2026, "name": "demo", "bduss": "demo-bduss", "stoken": "demo", "workdir": "/"}
        ],
        "appid": 266719,
        "pcs_addr": "pcs.baidu.com",
        "pan_ua": "netdisk;7.42.0.5;PC;PC-Windows;10.0.22631;WindowsBaiduYunGuanJia",
        "enable_https": False,
        "proxy": f"http://127.0.0.1:{PORT}",
    }
    (home / "pcs_config.json").write_text(json.dumps(config, ensure_ascii=False))
    (home / "ui_config.toml").write_text(
        # Never start a real player: its requests would bypass the fake proxy.
        '[download]\nconcurrency = 8\n[player]\ncommand = "true"\n'
    )


def record(name, script, cast_path: Path):
    home = Path(tempfile.gettempdir()) / "baidupcsr-demo"
    shutil.rmtree(home, ignore_errors=True)
    home.mkdir()
    pcs_config(home)
    env = {
        "PATH": os.environ["PATH"],
        "HOME": str(home),
        "TMPDIR": str(home),
        "TERM": "xterm-256color",
        "COLORTERM": "truecolor",
        "LANG": "C.UTF-8",
        "BAIDUPCS_GO_CONFIG_DIR": str(home),
        "BAIDUPCSR_UI_CONFIG": str(home / "ui_config.toml"),
        "BAIDUPCSR_CACHE_DIR": str(home / "cache"),
        "BAIDUPCSR_DOWNLOAD_DIR": str(home / "Downloads"),
    }
    master, slave = os.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
    child = subprocess.Popen(
        [str(BINARY)],
        stdin=slave, stdout=slave, stderr=slave, env=env, cwd=home,
        start_new_session=True,
        preexec_fn=lambda: fcntl.ioctl(0, termios.TIOCSCTTY, 0),
    )
    os.close(slave)

    decoder = codecs.getincrementaldecoder("utf-8")("replace")
    events, start = [], time.monotonic()

    def pump(seconds, keep=True):
        deadline = time.monotonic() + seconds
        while (remaining := deadline - time.monotonic()) > 0:
            ready, _, _ = select.select([master], [], [], remaining)
            if not ready:
                continue
            try:
                data = os.read(master, 65536)
            except OSError:
                return
            # Answer the image-protocol probe as a plain terminal would, so
            # the app settles on half-block pictures right away.
            if b"\x1b[c" in data:
                os.write(master, b"\x1b[?62;22c")
            if b"\x1b[5n" in data:
                os.write(master, b"\x1b[0n")
            text = decoder.decode(data)
            if text and keep:
                events.append([round(time.monotonic() - start, 4), "o", text])

    for keys, pause in script:
        if keys:
            os.write(master, keys.encode())
        pump(pause)
    # Quit off the record, so the GIF loops without a blank frame.
    os.write(master, b"\x03")
    pump(0.3, keep=False)
    if child.poll() is None:
        os.write(master, b"y")
        pump(0.5, keep=False)
    if child.poll() is None:
        os.killpg(child.pid, signal.SIGTERM)
    child.wait(timeout=5)
    os.close(master)
    shutil.rmtree(home, ignore_errors=True)

    header = {"version": 2, "width": COLS, "height": ROWS, "env": {"TERM": "xterm-256color"}}
    with cast_path.open("w") as cast:
        cast.write(json.dumps(header) + "\n")
        for event in events:
            cast.write(json.dumps(event, ensure_ascii=False) + "\n")


def main():
    names = sys.argv[1:] or list(SCENES)
    agg = os.environ.get("AGG") or shutil.which("agg") or "agg"
    out = HERE / "out"
    out.mkdir(exist_ok=True)
    GIF_DIR.mkdir(parents=True, exist_ok=True)
    server = subprocess.Popen(
        [sys.executable, str(HERE / "fake_pcs.py"), "--port", str(PORT)],
        stdout=subprocess.DEVNULL,
    )
    try:
        time.sleep(0.8)
        for name in names:
            cast = out / f"{name}.cast"
            print(f"recording {name}…", flush=True)
            record(name, SCENES[name], cast)
            subprocess.run(
                [
                    agg, str(cast), str(GIF_DIR / f"{name}.gif"),
                    "--font-family", "JetBrainsMono Nerd Font Mono,Noto Sans Mono CJK SC",
                    "--font-size", "15", "--line-height", "1.2", "--theme", "github-dark",
                    "--idle-time-limit", "3", "--last-frame-duration", "2", "--fps-cap", "20",
                ],
                check=True,
            )
    finally:
        server.terminate()


if __name__ == "__main__":
    main()
