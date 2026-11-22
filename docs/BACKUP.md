# 备份与恢复（0.6.0 起）

## 备份位置

```
~/Backups/Eryu/2026-11-19-v0.6.0/
├── Eryu-0.6.0.zip              发布的 App（9.3M，签名 Eryu Dev）
├── Eryu-0.6.0.app              解包后的 App（可直接跑 `--self-test` 验证）
├── eryu-repo-v0.6.0.bundle     完整 git 历史（含 v0.6.0 tag，25M）
├── SHA256SUMS                  上面两个文件的校验和
├── DESIGN.md / CHANGELOG.md    设计快照与版本记录
```

校验：

```sh
cd ~/Backups/Eryu/2026-11-19-v0.6.0 && shasum -a 256 -c SHA256SUMS
```

## 恢复

```sh
# 1) 恢复源码（到任意空目录）
git clone ~/Backups/Eryu/2026-11-19-v0.6.0/eryu-repo-v0.6.0.bundle eryu
cd eryu && git checkout v0.6.0

# 2) 只恢复可运行版本
unzip -q ~/Backups/Eryu/2026-11-19-v0.6.0/Eryu-0.6.0.zip -d /tmp/eryu-restore
cp -R /tmp/eryu-restore/Eryu.app /Applications/

# 3) 用户数据（不在备份里，按需自行拷贝）
#    ~/.eryu/{config.toml,hotwords.toml,profiles.toml,history.json,stats.json,models/}
```

## 每次冻结版本的固定动作

1. `ERYU_GATE_FAST=1 ./scripts/dist.sh`（闸门 9 项 → 构建 → 签名 → zip）
2. `./scripts/install.sh`（装到 `/Applications`，同一时间只有一个 Eryu）
3. `git commit` + `git tag -f vX.Y.Z`
4. 在 `~/Backups/Eryu/<日期>-vX.Y.Z/` 放：zip、app、`git bundle --all`、`SHA256SUMS`、`DESIGN.md`
5. 用 `git clone <bundle>` 和 `--self-test` 各验一次（不接受"应该没问题"）
