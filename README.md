# baidupcsr

A terminal file manager for Baidu Netdisk (百度网盘). It reuses the login from
[BaiduPCS-Go](https://github.com/qjfoidnh/BaiduPCS-Go): log in there once, then run `baidupcsr-cli`.

- Miller-column browser: parent │ current │ preview
- Streams videos straight into mpv, with seeking and no local proxy
- Parallel, resumable downloads (`.part` files)
- Text previews and image thumbnails (kitty, sixel, iTerm2 or half-blocks), recursive search, copy/move/rename/delete

## Usage

```
baidupcsr-cli               # open the browser
baidupcsr-cli doctor        # check config and login (prints no secrets)
baidupcsr-cli completions zsh > _baidupcsr-cli
```

Press `?` inside the app for all keys. The essentials:

| Keys | Action |
| --- | --- |
| `h j k l` / arrows | navigate, `l`/`⏎` opens or plays |
| `Space` `v` `V` | mark, visual range, mark all |
| `D` | download marked files (or the one under the cursor) |
| `y` `x` `p` | yank, cut, paste into the current directory |
| `dd` `r` `a` | delete, rename, new directory |
| `/` `s` `:` | filter, search below here, command (`mkdir`, `mv`, `cp`, `rm`, `search --api`) |
| `S` `T` `R` | sort, tasks, refresh |

## Configuration

`~/.config/baidupcsr/ui_config.toml`. Every key is optional:

```toml
[ui]
icons = "nerd"        # or "ascii" without a Nerd Font

[download]
dir = "/data/downloads"  # default: BaiduPCS-Go's savedir, then the system downloads dir
chunk_size = 4194304
concurrency = 8

[network]
proxy = "socks5://127.0.0.1:1080"

[player]
command = "mpv"
args = ["--fs"]
```

Environment overrides: `BAIDUPCS_GO_CONFIG_DIR`, `BAIDUPCSR_UI_CONFIG`, `BAIDUPCSR_CACHE_DIR`,
`BAIDUPCSR_DOWNLOAD_DIR`, `BAIDUPCSR_PROXY`, and `NO_COLOR`.
