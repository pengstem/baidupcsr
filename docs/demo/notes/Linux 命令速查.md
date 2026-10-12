# Linux 命令速查

## 文件
| 命令 | 作用 |
| --- | --- |
| `ls -lah` | 详细列出，含隐藏文件 |
| `du -sh *` | 每个条目占用空间 |
| `find . -name '*.mkv'` | 按名字查找 |
| `rsync -avP src/ dst/` | 增量同步，显示进度 |

## 进程
- `htop`：交互式查看进程
- `kill -TERM <pid>`：先礼后兵，不行再 `-KILL`

## 网络
```bash
ss -tlnp          # 监听中的端口
curl -I https://example.com
```
