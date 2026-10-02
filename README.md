# locallease — 完全本地的 Lease 分布式锁模拟器

一个纯本地的分布式锁（lease lock）模拟器：多个客户端以 goroutine 形式在同一进程内竞争锁，
锁状态、租约、fencing token 与审计日志只保存在**本地内存或本地文件**中，
不依赖 Redis / etcd / ZooKeeper 或任何外部服务。

## 目录结构

```
lease/      核心库：Clock、Lease、Manager、Store（内存/文件）、FencedStore、审计日志
client/     模拟客户端（goroutine），支持 Pause/Resume 模拟进程冻结
cmd/demo/   可运行演示：暂停-过期-接管-fencing 拒绝 的完整流程
```

## Lease 状态机

每个资源（resource）在任意时刻**最多存在一个有效 lease**。单个 lease 的状态流转：

```
                 Acquire 成功
        (无 lease 或旧 lease 已过期)
                      │
                      ▼
              ┌───────────────┐   Renew 成功(未过期, holder+token 匹配)
              │     VALID     │ ──────────────────────────┐
              │ [now, ExpiresAt) │◀────────────────────────┘ (ExpiresAt = now+TTL)
              └──────┬────────┘
        Release 成功 │            │ clock.Now() >= ExpiresAt
                     ▼            ▼
              ┌───────────────────────┐
              │   FREE / EXPIRED      │  可被任何人 Acquire 接管（产生新 token）
              └───────────────────────┘
```

规则：

- `Acquire(resource, holder, ttl)`：仅当不存在未过期 lease 时成功；成功即铸造新的 fencing token。
- `Renew(resource, holder, token, ttl)`：holder 与 token 必须匹配当前 lease，且 `now < ExpiresAt`；
  **已过期的 lease 永远不能续期**。
- `Release(resource, holder, token)`：holder 与 token 匹配才删除 lease，否则为失败空操作。
- 所有状态转移在单个互斥锁下串行化，并**先持久化、后提交内存**。

## Fencing token 原理

仅有过期时间不足以保证安全：持有者可能因 GC 停顿/网络分区“睡过”了自己的租约，
醒来后锁已被别人接管，若它继续写下游资源就会造成脑裂。解决方案是 fencing token：

1. 每次 `Acquire` 成功，管理器颁发一个**严格递增**的 token（全局计数器，随状态一起持久化，
   重启后也不会回退）。
2. 持有者访问下游资源时必须携带自己的 token。
3. 下游资源（本工程用 `lease.FencedStore` 模拟）只接受**大于已见最大 token** 的写请求。

因此旧持有者醒来后，它的 token 必然小于新持有者的 token，写请求会被拒绝：

```
alice: Acquire → token=1 ── 暂停 ──────────→ 醒来: Write(token=1) ✗ 被拒绝 (stale)
                              bob: Acquire → token=2, Write(token=2) ✓
```

`client.Client.Write` 演示了携带 token 访问 `FencedStore` 的完整路径。

## 时间模型

- 所有时间判断都通过可注入的 `lease.Clock` 接口（`Now() time.Time`）完成。
- 生产/demo 使用 `RealClock`；测试使用 `FakeClock`，通过 `Advance(d)` 手动推进时间。
- **测试不依赖任何真实 sleep**：租约过期、暂停超时等场景都由 FakeClock 确定性触发。
- lease 有效期为左闭右开区间 `[AcquiredAt, ExpiresAt)`：到达 `ExpiresAt` 即视为过期。

## 持久化与重启

- `lease.FileStore` 将 `{leases, lastToken}` 以 JSON 原子写入本地文件（临时文件 + rename）。
- 重启后用同一文件构造新的 `Manager`：
  - 未过期的 lease 正常恢复（持有者可续期，他人无法获取）；
  - **已过期的 lease 不会被“复活”**——加载时不刷新过期时间，只在操作时按当前时钟惰性判定，
    因此新持有者可立即接管，旧持有者续期失败；
  - token 计数器持久化，重启后 token 仍然单调递增。

## 运行测试

```bash
go test ./...            # 全部测试
go test -race -count=1 ./...   # 含竞态检测
go test -v ./lease ./client    # 查看单个场景
```

测试覆盖（全部使用 FakeClock，无真实 sleep）：

| 场景 | 测试 |
|---|---|
| 竞争获取（并发仅一个赢家） | `TestConcurrentAcquireExactlyOneWinner` / `TestConcurrentClientsSingleWriter` |
| 续约延长过期时间 | `TestRenewExtendsExpiry` |
| 错误 holder/token 续期、释放被拒绝 | `TestRenewWithWrongHolderOrTokenFails` 等 |
| 租约超时后不可续期、可被接管 | `TestExpiredLeaseCannotBeRenewed` / `TestExpiredLeaseTakeover` |
| 旧客户端暂停恢复 + fencing 拒绝 | `TestPausedClientLosesLeaseAndIsFencedOut` |
| 短暂暂停后仍可续期 | `TestShortPauseKeepsLease` |
| token 严格单调（含跨资源、跨重启） | `TestFencingTokenStrictlyIncreasing` / `TestTokenCounterSurvivesRestart` |
| 释放后重获 | `TestReleaseAndReacquire` / `TestRestartAfterRelease` |
| 重启恢复/不复活过期 lease | `TestRestartPreservesValidLease` / `TestRestartDoesNotResurrectExpiredLease` |
| 下游 fencing 存储拒绝旧 token | `TestFencedStoreRejectsStaleTokens` |

## 运行演示

```bash
go run ./cmd/demo -state /tmp/lease-state.json -log /tmp/lease-log.jsonl
```

输出示例：

```
alice acquire ok=true token=1
bob acquire ok=true token=2
alice renew after pause: ok=false (expected failure)
alice stale write rejected: stale fencing token 1 (already processed token 2)
alice re-acquire ok=true token=3 (monotonic)
```
