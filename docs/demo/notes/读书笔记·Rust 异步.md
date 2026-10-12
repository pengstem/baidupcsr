# 读书笔记：Rust 异步编程

> 取消不是异常，而是一种正常的控制流。

## Future 与取消
- `Future` 被 drop 即被取消，**没有**析构之外的回调。
- 用 `CancellationToken` 把取消显式化：父 token 取消时，所有子 token 一起取消。
- `tokio::select!` 加 `biased;` 让取消分支优先被检查。

```rust
tokio::select! {
    biased;
    () = cancel.cancelled() => Err(Cancelled),
    result = request.send() => result,
}
```

## 背压
- 滑动窗口：`FuturesOrdered` 最多保持 N 个请求在途；
- 按顺序落盘，保证 `.part` 永远是一段连续前缀，断点续传只需看文件长度。
