# 固件 PR review 的容器入口

`review-pr.sh` 在 Buildkite agent 中运行：读取 PR metadata、准备 Git 提交，再通过宿主机 Docker 启动独立 review 容器。工具和规则在 `keystone-ai-review` 镜像内。此入口使用 Docker CLI，无需将 Docker socket、AWS/GitHub 凭据或 SSH agent 传给 review 容器。

入口分工：

- `review-pr.sh`：参数检查、curl 获取 PR、Git 准备、Docker 启停、日志和 Buildkite annotation 上传。
- `pr-context.py`：解析 PR JSON、冻结输入信息、生成 background。
- `publish-review.py`：校验 review 结果与当前 PR head，以一个 advisory GitHub review 回写 findings。
- `review-summary.py`：检查结果是否完成，生成 `status.json` 和英文 CI 摘要。模型生成的审查报告仍为中文。
- `model-config.py`：从解密后的 `KEY=value` 文本中提取 GitHub token，或生成 OCR 配置，不执行文件中的内容。

Agent 需要 Bash、Git、curl、Python 3 和 Docker CLI；Node/OCR 只在 review 镜像中安装。

## 必需配置

| 环境变量 | 含义 |
| --- | --- |
| `REVIEW_REPOSITORY` | GitHub `owner/name`，对应本固件仓库 |
| `REVIEW_PROVIDER` | OCR 中的 provider 名称 |
| `REVIEW_MODEL` | 本次模型 ID；使用密文 key 文件时自动写入 OCR provider 的 `model` 和 `models` |
| `REVIEW_EFFORT` | OCR 审查 effort：low / medium / high，不是模型推理强度 |
| `GH_TOKEN` 或 `GITHUB_TOKEN` | 可选：覆盖密文文件中的 GitHub token；需要 Pull requests read/write 权限 |

模型 key 和 GitHub token 默认一起读取仓库中的 `ci/ai-review/secrets.env.encrypted`，格式和加密方法见下一节。无需在 Buildkite 页面单独设置 token。已有环境变量 `GH_TOKEN` / `GITHUB_TOKEN` 优先于密文中的 GitHub token；也可以通过 `OCR_CONFIG_JSON` 或 `OCR_CONFIG_FILE` 提供完整 OCR 配置来覆盖模型 key，二者不能同时设置。两类凭据分别覆盖：只提供 OCR 配置时，仍可从密文读取 GitHub token。预检不需要模型凭据，但读取实时 PR 仍需要 GitHub token。

可选配置：`REVIEW_PR` 覆盖 Buildkite PR 编号；`REVIEW_BASE`、`REVIEW_HEAD` 指定历史提交；`REVIEW_PREVIEW=true` 只做预检；`REVIEW_PUBLISH=false` 禁止回写 GitHub，完整审查默认回写；`REVIEW_DEBUG=true` 保留全部中间文件并导出时间、token 和计划诊断，默认关闭；`REVIEW_PASS`、`REVIEW_MODE`、`REVIEW_CONCURRENCY`、`REVIEW_TIMEOUT`、`OCR_LLM_TIMEOUT` 对应 runner 参数。未给出模型推理字段时沿用 provider 默认。

镜像默认使用 `623147552995.dkr.ecr.eu-west-1.amazonaws.com/code-review-agent-image:latest`，每次运行先执行 `docker pull`。可用 `REVIEW_IMAGE` 覆盖镜像地址，`REVIEW_PULL_IMAGE=false` 可用于本地已准备好的镜像；实际镜像 ID 和 digest 会保存在 `image.json`。

拉取和运行默认指定 `linux/amd64`，匹配当前 AWS agent。可用 `REVIEW_PLATFORM` 覆盖目标平台。ECR 镜像必须实际包含该架构；只加运行参数不会把 ARM 镜像转换为 amd64。遇到 `Exec format error` 时，在工具仓库运行修正后的 `bash auto/release`，重新构建并发布 amd64 的 `latest` 后再重试。

Review step 使用 `ecr#v2.3.0` 插件在执行脚本前登录 `eu-west-1` 的 ECR，沿用现有固件构建的插件版本。AWS 凭据和镜像拉取权限由 agent 环境提供。脚本通过 agent 内已有的 `shush` 调用 KMS 解密密文文件。`OCR_CONFIG_FILE` 也在 agent 侧读取，再仅向 Docker 传入配置环境变量，因此不用把 agent 私有文件路径挂到宿主机 Docker。

## Unclustered agent 的凭据配置

当前 agent 是 Unclustered，沿用基础设施已有的 AWS KMS + `shush` 方式，不使用 Buildkite 原生 `secrets:` 字段。现有加密脚本使用 `eu-west-1` 的 `alias/keystone`。本地身份需要该 key 的加密权限，agent 的 task role 需要解密权限；实际权限以 KMS 返回为准。

明文文件为 `ci/ai-review/secrets.env`，格式如下。保留字段名，把占位值替换为实际 key。已有文件只需补上 `GITHUB_TOKEN`，保留原来的模型 key：

```dotenv
OPENAI_APIKEY=xxxx
GITHUB_TOKEN=xxxx
```

GitHub token 需要目标仓库的 Pull requests read/write 权限，用于读取 PR 和发布 review。已有环境变量可覆盖它，优先级为 `GH_TOKEN`、`GITHUB_TOKEN`、密文文件中的 `GITHUB_TOKEN`。

首次配置可从 `secrets.env.example` 复制。`secrets.env` 已加入本目录的 `.gitignore`，Git 中只提交加密后的 `secrets.env.encrypted`。轮换 key 时重新加密并提交新的密文文件。

在已有 AWS 凭据的本地终端执行以下命令。`auto/shush` 会向 Docker 转发 `AWS_ACCESS_KEY_ID`、`AWS_SECRET_ACCESS_KEY` 和 `AWS_SESSION_TOKEN`，不会自动加载宿主机的 AWS profile。加密成功后才替换最终文件：

```bash
cd /Users/chendong/keystone/keystone3-firmware
AWS_DEFAULT_REGION=eu-west-1 \
  bash ../buildkite-agent-staging/auto/shush encrypt alias/keystone \
  < ci/ai-review/secrets.env \
  > ci/ai-review/secrets.env.encrypted.tmp &&
mv ci/ai-review/secrets.env.encrypted.tmp ci/ai-review/secrets.env.encrypted
```

密文必须由 `shush encrypt` 生成，不能直接换成 `aws kms encrypt` 的输出。把 `secrets.env.encrypted` 和 CI 代码一起提交并合入 `master`（默认的 `REVIEW_CI_BRANCH`）。Buildkite 从该分支读取脚本和密文；本地直接运行 `review-pr.sh` 时读取脚本同目录的密文。无需设置 `KMS_ENCRYPTED_OCR_CONFIG` 或 `KMS_ENCRYPTED_GITHUB_TOKEN`，也无需新增 agent hook。若已按旧方案配置 GitHub token 的 hook，移除该段及其密文变量；同时删除旧的明文 token 环境变量，才能使用文件中的新 token。无需为本次修改重建 agent 或 review 镜像。

提交和推送后，在对应分支的新提交上新建手动构建，沿用成功预检的设置，并设置：

```text
REVIEW_PREVIEW=false
REVIEW_PR=98
REVIEW_PROVIDER=openai-responses
REVIEW_MODEL=gpt-5.6-sol
REVIEW_EFFORT=medium
REVIEW_PASS=both
REVIEW_MODE=chained
```

脚本在读取 PR metadata 之前按需解密文件，每次运行最多解密一次。`GITHUB_TOKEN` 导出为 agent 侧的 `GH_TOKEN`，供读取 PR 和回写 review 使用；`OPENAI_APIKEY` 组装成 `OCR_CONFIG_JSON` 传给 review 容器，GitHub token 不传入容器。脚本不把明文写入日志或宿主机产物。空行和 `#` 注释可用，值可以加成对的单引号或双引号；文件按文本解析，不进行 Shell 展开或命令执行。容器入口校验 JSON 并创建内部配置文件。该 key 文件的模型配置支持 `openai` 和 `openai-responses` provider；本次 GPT 配置使用 `openai-responses`，自定义 endpoint 或其他 provider 可通过完整 OCR 配置传入。

`REVIEW_MODEL` 同时写入所选 provider 的 `model` 和 `models`。OCR 会用内置模型列表加上 `models` 校验 `--model`；只提供 API key 会使未收录的新模型在请求 API 前报错。显式登记模型解决的是 OCR 本地校验，API 端的模型可用性仍由服务端决定。推理参数使用 provider 默认值，`REVIEW_EFFORT=medium` 控制 OCR 的审查力度。

日志会显示实际使用的 provider、model 和 effort，空格等特殊字符会转义显示。配置生成失败与 KMS 解密失败分别报告；provider 不支持时会给出实际值。需要从文件读取的凭据缺失或解密失败时，任务在启动 review 容器前失败。预检不调用模型；使用本地 `REVIEW_PR_JSON` 或已提供 GitHub token 时，预检也不调用 KMS，否则会解密获取 GitHub token。旧的仅含模型 key 的密文文件仍可配合环境变量中的 GitHub token 使用。

## Buildkite 接入

当前使用关联固件仓库的 `keystone3-firmware-internal-code-review` pipeline。bootstrap 和 review 步骤都选择 `keystone-staging:build` 队列。

PR webhook 会 checkout PR 的提交，pipeline 的默认分支不会替代 PR checkout。已有 PR 分支可能没有 `ci/.buildkite/ai-review.yml`，所以 bootstrap 从 `master` 的最新版本加载 YAML，review 步骤再从该分支提取 Shell 入口、Python helper 和加密 key 文件。`REVIEW_CI_BRANCH` 默认是 `master`；仅在合并前手动测试时可覆盖为 `ai-review`。审查使用的 checkout 和 `BUILDKITE_COMMIT` 始终对应原 PR，base/head 仍从 PR metadata 获取；工具和规则仍使用 ECR 的 `latest`。无需维护固定的 CI commit。

启用步骤：

1. 将本次 CI 修改随 `ai-review` 分支合并到 `master`。将 Buildkite pipeline 的默认分支也改为 `master`，用于后续手动构建。
2. 在 Buildkite pipeline 的 **Settings → Steps** 中，将 bootstrap 的 command 替换为 [`../.buildkite/ai-review-bootstrap.sh`](../.buildkite/ai-review-bootstrap.sh) 的完整内容。不能只填写 `bash ci/.buildkite/ai-review-bootstrap.sh`：PR 分支可能连这个文件也没有。保留 `queue=keystone-staging:build`。
3. 删除旧的 `REVIEW_CI_REVISION`。自动构建不要设置固定的 `REVIEW_PR`、`REVIEW_BASE`、`REVIEW_HEAD` 或 `REVIEW_PR_JSON`；PR 编号来自 webhook。手动构建仍可单独设置 `REVIEW_PR`。
4. 在 **Settings → GitHub** 中启用 **Build when pull request is opened or updated** 和 **Build when pull request becomes ready for review**。关闭 **Build branches**、**Build tags**、**Build when pull request is reopened**、**Allow builds from third-party forked repositories** 和 **Skip when pull request has existing build for commit and branch**。保留 **Skip when pull request is closed or merged**。
5. 在同一页面启用 **Filter builds using a conditional**，填写下面的条件。Buildkite 将 opened 和 updated 放在同一个开关中，必须配合事件过滤才能保留新建 PR、排除后续 push。
6. 新建 PR，或将 draft PR 转为 ready。确认自动完成 review。随后向 PR 推送新提交，确认不再自动审查。

```text
build.source_event == "pull_request" &&
(
  build.source_action == "opened" ||
  build.source_action == "ready_for_review"
)
```

自动审查仅响应 PR 新建（包括 draft）和转为 ready；push、synchronize、reopened 和评论事件不自动审查。后续需要复审时，在 Buildkite 选择 **New Build**，branch 设为 `master`、commit 设为 `HEAD`，并为这次 build 设置 `REVIEW_PR=108`（替换成目标 PR 编号）。手动构建使用当前 PR 的 base/head，继续沿用 `medium`、`both`、`chained`。

这些事件由 Buildkite 已有的 GitHub webhook 处理，无需添加 GitHub Actions workflow，也无需在 GitHub 中新增 Buildkite API token。页面条件在创建 webhook build 前过滤事件；YAML 也限制 review step 的事件。`review-pr.sh` 在读取凭据和启动模型之前再检查事件，让仍使用旧 YAML 的 PR 也能跳过 push。只更新代码而未保存页面条件时，push 仍可能产生一个跳过审查的 build，但不会调用模型。手动构建不受 webhook 事件过滤影响。

同一 head 转为 ready 或手动新建 build 时可以再次审查并发布。只有同一个 build 对同一 PR/head 的重复发布会被跳过。

仅修改仓库文件不会更新 Buildkite 页面中保存的 bootstrap command 或 GitHub 开关；需要在页面保存一次。之后每次运行自动获取 `REVIEW_CI_BRANCH` 的最新 CI 文件。触发机制见 [Buildkite GitHub 文档](https://buildkite.com/docs/pipelines/source-control/github)。

YAML 为自动触发提供以下默认配置：`openai-responses`、`gpt-5.6-sol`、`medium`、`both`、`chained`，并开启回写。第二轮 blocker 审查接收第一轮 bug findings；如果加入 findings 后 background 超限，就省略 findings。通过 Buildkite 手动创建 build 时，可以传入同名变量覆盖这些值；pipeline upload 会把最终值写入 review step。

完整审查成功后，脚本使用 `COMMENT` 类型提交一个 GitHub review，不会 approve 或 request changes。能确认属于当前 diff 的位置会成为行内评论，其余 findings 会放在 review body。正文包含实际 provider、model 和本次所有审查轮次的输入、输出、总 token 数；缓存用量单独展示，不重复加到总数。模型信息读取 `review/run.json`，token 数直接读取汇总 `review/report.json` 的 `tokens`，缺失时显示 `not recorded`，不用当前环境变量猜测，也不把缺失记成 0。行内评论被 GitHub 拒绝而退回正文发布时，仍保留这些信息。

发布前会重新读取 PR：如果 PR 已关闭或 head 在模型运行期间发生变化，就跳过旧结果。去重匹配 repo、PR、head marker 和完整 `BUILDKITE_BUILD_URL`：同一个 build 重试不会重复发布，新的 build 可以在相同 head 上发布新的审查结果，适用于修复后复审和换模型复测。已有旧 review 中的 Buildkite 链接同样参与匹配。直接在本地发布、未设置 `BUILDKITE_BUILD_URL` 时，保留按 PR/head 去重；从历史 build 的产物补发时，应设置原 build 的 URL。

发布结果及模型/token 信息写入 `publish.json`。Buildkite annotation 同时显示发布状态、review 链接和跳过原因。

当前仍是 shadow 阶段：Buildkite check 和 GitHub review 都不设为 required，也不决定是否允许合并。

## 本地容器预检

在本固件仓库执行下面的形态，变量需先设置。`REVIEW_PR_JSON` 可指向已有 GitHub pull-request REST JSON 响应，以避免请求 GitHub；预检容器本身禁用网络，不需要模型凭据。

```bash
REVIEW_PREVIEW=true \
REVIEW_PULL_IMAGE=false \
REVIEW_IMAGE=keystone-ai-review:local \
REVIEW_REPOSITORY="$review_repository" \
REVIEW_PR="$review_pr" \
REVIEW_PROVIDER="$review_provider" \
REVIEW_MODEL="$review_model" \
REVIEW_EFFORT="$review_effort" \
bash ci/ai-review/review-pr.sh
```

目标必须是包含 `.git` 目录的独立 checkout。构建中代码和输出均位于宿主机与 agent 共享的 `/buildkite/builds`；容器将代码只读挂到 `/workspace`，本次输出单独挂到 `/output`。

每次运行使用 `tmp/ai-review/<独立ID>/`，保存 PR 输入、镜像身份、容器日志、发布状态、英文 CI 摘要和工具产物。工具默认保留汇总及两轮报告、日志、配置和输入快照；成功后清理临时预检和计时文件，失败时保留已有中间文件。Buildkite 的 `artifact_paths` 在命令结束后收集产物，再进入 agent 的 `pre-exit` 清理。如果 Spot 实例被直接终止，本地尚未上传的产物可能丢失。取消时脚本停止它创建的容器。

运行失败与零 findings 分开记录；找到 blocker 本身不使 shadow 任务失败。报告正文不包含时间、plan 或 token 分析。

## 当前边界

2026-09-30：Build #42 已跑通两轮审查、KMS 凭据解密和 GitHub review 回写；Build #52 已确认自动构建使用 PR #108 的 base/head，并从 master 加载 CI 脚本。本次将自动审查限制为 opened / ready_for_review，事件过滤合入 master 并保存页面设置后需验证一次。当前结果仍只用于 shadow 对比，不接管合并门禁。
