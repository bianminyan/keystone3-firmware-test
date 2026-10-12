# 内部源码导出到公开仓库

从仓库根目录运行，需要 Python 3 和 Git。脚本只处理已提交的源码，使用独立临时 Git 索引，不切换当前分支、不修改工作区或暂存区，也不初始化子模块。未提交的修改不会进入导出结果。

## 准备

1. 更新内部 release 分支并完成测试、版本号及中英文 changelog。
2. 将公开侧新增修改合入或整理到内部 release；冲突在导出前解决。
3. 配置 `public` remote 指向公开固件仓库，将本地 `main` 对齐公开 `master`。
4. 导出会清理相对公开基线新增的注释；源码分支保持不变。删除注释行会影响行号，候选必须重新构建验证 hash。

所有命令都会使用固定 SHA。`check`、`create` 会 fetch 公开基线，但不会自动 pull 内部 release，也不会替你更新 main。

```bash
python3 ci/release_export.py check --source release/v3.1.0 --online-base 3.1.2
python3 ci/release_export.py create --source release/v3.1.0 --online-base 3.1.2 --candidate 3.1.0-rc3 --version 3.1.0
python3 ci/release_export.py verify --candidate 3.1.0-rc3
```

默认公开基线是 `public` remote 的 `master`，可以用 `--remote` 和 `--public-branch` 指定。若本地 main 存在但未对齐公开基线，脚本停止。

必须用 `--online-base` 指定已确认同步到内部的当前线上版本，支持 `public/master`、public tag（例如 `3.1.2`）、`refs/heads/<branch>` 或完整 commit SHA。脚本从 public 获取并固定该版本，确认它属于公开 master 历史。只检查该版本到最新公开 master 之间新增的修改，不再追溯内外部分支的共同祖先。若这些后续公开修改在源码快照中不一致，默认停止并列出路径。人工确认差异已经正确处理后，才能在 `check` / `create` 中加 `--ack-public-base`；这不是自动合并，源码快照仍会覆盖这些路径。该确认及路径清单会写入映射记录。

## 导出规则

- 源码、版本、changelog、docs、第三方文件来自固定的内部 release commit。
- 排除 `ci/`、`.github/CODEOWNERS`、`.github/copilot-instructions.md`、`.github/instructions/`、`.github/workflows/copilot-review-gate.yml`。
- 保留 `.github/workflows/on-release.yml` 的源码版本。
- 根 `.gitignore` 和 `keystone3-firmware-release` gitlink 来自固定的公开基线。
- `.gitmodules` 以源码为基础，恢复公开基线中的 release 子模块完整配置；URL 必须为 `git@github.com:KeystoneHQ/keystone3-firmware-release.git`。
- 新增注释清理规则见下文。新的内部专用目录必须先加入排除规则并升级规则版本。

候选只有一个新提交，父提交为公开基线，提交信息是 `release <version>`，不会引入内部提交历史。已有候选或记录不会被覆盖。源码更新后请用新候选名。

## 映射与验证

本地 annotated tag `internal-export/<candidate>` 保存 JSON，包含源码 SHA、公开基线 SHA、候选 SHA、导出 tree、公开 release 子模块指针、规则版本、远端地址和人工确认的差异。

`verify` 重新构造期望 tree，核对内容、父提交及 tag 指向，并对“内部源码 → 导出结果”执行 `git diff --check`，防止导出操作引入空白错误；源代码或生成资源已有的空白不会被自动格式化或作为导出失败条件。候选被手工修改后验证会失败，应修改源码或规则，再创建新候选。记录与候选不是加密签名或独立审计证明。

脚本不编译固件，记录中明确标记 `firmware_build: not_run`。需要构建验证时，可显式创建工作区：

```bash
git worktree add /path/to/rc3-check 3.1.0-rc3
```

在候选工作区执行仓库已有构建、测试及 hash 校验。不要把结构验证通过理解为固件 hash 相同。

## 明确推送

确认候选及构建产物后才执行：

```bash
python3 ci/release_export.py publish --candidate 3.1.0-rc3
```

该命令再次验证并检查公开基线是否前移，将候选分支和内部映射 tag 原子推送到内部 `origin`。拒绝覆盖不同内容的同名分支或 tag，不 force、不推 main/master、不向 public 写入。推送可能触发内部已有 CI。

确认内部候选后，由人工执行下面的命令推送到 public，并创建面向公开 master 的 PR。禁止向 public 推内部映射 tag。手动推送前重新检查公开基线是否变化。

```bash
git fetch origin refs/heads/3.1.0-rc3
git -c push.followTags=false push --no-follow-tags public FETCH_HEAD:refs/heads/3.1.0-rc3
```

## 脚本回归测试

```bash
python3 -m unittest discover -s ci/tests -p 'test_release_export.py'
```

测试使用临时本地仓库和 bare remote，不访问或推送实际 GitHub 仓库。

## Buildkite 自动导出

在内部 Buildkite 选择 GitHub 分支，构建消息填写 `export-release`。随后输入当前线上版本、候选分支名和目标版本号；无需填写源码分支。任务固定使用 `BUILDKITE_COMMIT`，而不是再次读取分支头。

流程为：隔离克隆 → create → verify → publish 原子推送候选分支和映射 tag 到 internal。没有额外的推送审批，也不构建固件或创建正式 release。人工同步到 public 后，再进行公开侧构建和 PR 审核。导出 JSON 会作为 Buildkite artifact 保存。

准备要求：

- agent 安装 Git、Python 3、Buildkite agent，并配置 GitHub SSH 认证。
- agent 可从内部仓库读取源码，并向内部 origin 写候选分支和 `internal-export/*` tag。公开基线通过 HTTPS 只读获取，不需要 public 写权限。
- 当前使用现有 `keystone-g3/production-test-build` queue；本任务不申请 KMS 签名权限，也不读取 AWS 发布密钥。
- 先将这套脚本合入需要触发导出的源码分支；Buildkite 从选定 commit 加载脚本。

CI 会检查内部同名候选及映射 tag 是否存在；存在就停止，不覆盖、不自动使用 `--ack-public-base`。公开差异需要先在源码中处理，或按上面的本地流程人工确认。

内部候选与映射 tag 使用 `git push --atomic` 一起写入，避免只有其中一个成功。远端不支持 atomic 时直接失败，不降级成分开推送。任务在临时 clone 中操作，退出后删除临时目录。

脚本映射格式升级为规则版本 4，旧映射需重新导出。

## 候选注释清理

对比固定的 public 基线与内部源码，移除新增或整段改写的 C/C++、Rust、Python、CMake、链接脚本注释。独立注释行完整删除，行内注释只保留必要的代码分隔。注释清理仅修改导出树，不写源码工作区。

保留已有注释（包括移动的同文注释）、第三方许可证、工具指令和 shebang。整个 `external/` 及生成资源 `src/ui/gui_assets/` 不清理。Python docstring 是运行时字符串，此步骤保留，不当作普通注释删除。

混合历史行和新内容的多行注释会报错，需在源码中人工处理。Shell、YAML、TOML、JS/TS 的新增行若含疑似注释标记，保守停止，不冒险删除字符串或语法；这也可能要求人工处理含标记的字符串。Markdown 文档不参与清理。

`verify` 使用同样的清理算法重新生成预期树。移除注释会改变行号，不能据此保证固件 hash 与内部构建一致。

选择线上版本表示你已确认该版本的线上修复回迁到内部；脚本不会自动证明这件事。当所选版本等于最新 public master 时，没有后续公开修改需要拦截，内部新开发造成的文件差异可以正常导出。候选父提交、公开配置及注释清理对比仍使用最新 public master。映射记录同时保存 `online_ref`、`online_base` 和 `public_base`，避免混淆已上线版本与本次导出基线。

`--source` 是准备导出的内部代码；`--online-base` 是你已确认回迁完成的公开版本，不是内部代码。填 `public/master` 表示确认当前公开最新版已回迁，直接使用本次 fetch 的最新 SHA，不使用本地可能过时的跟踪引用，也不重复按 tag 拉取。此时不会再检查已上线历史是否遗漏，仍会执行导出内容验证。
