# 固件 PR review 的容器入口

`review-pr.sh` 在 Buildkite agent 中运行：读取 PR metadata、准备 Git 提交，再通过宿主机 Docker 启动独立 review 容器。工具和规则在 `keystone-ai-review` 镜像内。此入口使用 Docker CLI，无需将 Docker socket、AWS/GitHub 凭据或 SSH agent 传给 review 容器。

入口分工：

- `review-pr.sh`：参数检查、curl 获取 PR、Git 准备、Docker 启停、日志和 Buildkite annotation 上传。
- `pr-context.py`：解析 PR JSON、冻结输入信息、生成 background。
- `review-trigger.py`：读取触发构建的 PR 评论，核对命令及所属 PR，再启动修复复核。
- `publish-review.py`：校验 review 结果与当前 PR head，以一个 advisory GitHub review 回写 findings。
- `review_state.py`：读取同一发布账号的历史 review，恢复旧 findings 和原评论线程。
- `review_gate.py`：在 PR 当前 head 上发布独立合并状态，检查两轮审查和历史问题是否全部处理完成。
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
| `GH_TOKEN` 或 `GITHUB_TOKEN` | 可选：覆盖密文文件中的 GitHub token；需要 Pull requests read/write，启用门禁还需 Commit statuses read/write |

模型 key 和 GitHub token 默认一起读取仓库中的 `ci/ai-review/secrets.env.encrypted`，格式和加密方法见下一节。无需在 Buildkite 页面单独设置 token。已有环境变量 `GH_TOKEN` / `GITHUB_TOKEN` 优先于密文中的 GitHub token；也可以通过 `OCR_CONFIG_JSON` 或 `OCR_CONFIG_FILE` 提供完整 OCR 配置来覆盖模型 key，二者不能同时设置。两类凭据分别覆盖：只提供 OCR 配置时，仍可从密文读取 GitHub token。预检不需要模型凭据，但读取实时 PR 仍需要 GitHub token。

可选配置：`REVIEW_PR` 覆盖 Buildkite PR 编号；`REVIEW_BASE`、`REVIEW_HEAD` 指定历史提交；`REVIEW_PREVIEW=true` 只做预检；`REVIEW_PUBLISH=false` 禁止回写 GitHub，完整审查默认回写；`REVIEW_DEBUG=true` 保留全部中间文件并导出时间、token 和计划诊断，默认关闭；`REVIEW_PASS`、`REVIEW_MODE`、`REVIEW_CONCURRENCY`、`REVIEW_TIMEOUT`、`OCR_LLM_TIMEOUT` 对应 runner 参数。未给出模型推理字段时沿用 provider 默认。

## 修复后的复核

先更新并发布 `keystone-ai-review` 的镜像，再将本目录的修改放到 `REVIEW_CI_BRANCH` 指向的分支。沿用现有手动构建入口，增加以下环境变量：

```dotenv
REVIEW_PR=114
REVIEW_FIX=true
REVIEW_EFFORT=medium
REVIEW_PASS=both
REVIEW_MODE=chained
```

替换 PR 编号即可，无需填写 base/head。Provider 和 model 沿用现有配置；只看报告时同时设置 `REVIEW_GATE=false`、`REVIEW_PUBLISH=false`，`REVIEW_PREVIEW=true` 只检查输入和文件选择。两种方式仍需 GitHub token 读取 PR 和历史 review，但预检不需要模型 key。也可以按下文配置 PR 评论命令来触发。

流程为：

1. 从 GitHub 读取同一发布账号、同一 PR 的历史 AI reviews。新版 review 的隐藏 marker 保存稳定 finding ID、原评论位置和状态；旧格式从行内评论和 review 正文恢复。不把“这次没找到”当成修复，也不丢弃更早的未解决问题。
2. 用原生 OCR 逐条验证旧问题，输出 `fixed`、`open`、`unresolved`、`dismissed` 或 `out_of_scope` 及代码证据。未修改、已移动或删除的文件同样进入验证。无法得到完整有效结论时保留旧问题，报告为 partial，不向 GitHub 发布完成结果。
3. 上次 head 是当前 head 的祖先且 PR merge-base 相同时，继续对上次 head 到当前 head 的新增修改执行 bug → blocker。否则退回完整 PR 范围；没有历史记录时也执行完整 PR 审查。老格式没有保存 base，因此第一次复核采用完整范围。增量发现若与修复结论冲突，保留为待确认。
4. Reply to each existing finding thread with its verification result, and create inline comments for new findings. The review body lists only old findings that are open or unresolved. Fixed, dismissed, and out-of-scope findings stay in the full report and hidden history. If an original thread was deleted, keep its history and list the finding in the summary only while it remains open or unresolved. Reject replies to existing threads owned by another publisher or to non-root comments. Update available threads before publishing the new summary. Retries reuse this build's published review and do not repeat replies.

报告仍是中文，包含前后提交、每个旧问题的状态、证据、新发现、模型及总 token（复核 + 新增修改审查）。主报告在 `review/report.md`，机器结果在 `review/report.json`；`review/fix/` 保存复核结果，`review/incremental/` 保存后续代码审查。被当前规则排除的翻译/字体问题记为 out_of_scope，不会被误标为已修复。

Hidden review state uses the v2 marker with zlib compression and base64 encoding. All finding IDs, content, states, and thread links are kept. The reader also accepts existing v1 markers without compression. Publish the reader and writer changes together through `REVIEW_CI_BRANCH`; no review image rebuild is needed. The publisher does not set a local size limit for review bodies, inline comments, or fix replies. GitHub API errors are reported without dropping findings.

发布时仍检查 PR 当前 head，仍使用 `COMMENT`；不会自动 resolve 原线程或 approve PR。合并门禁使用下面的独立 commit status。GitHub 凭据仍在 agent 侧使用，不传给模型容器，不需要 Buildkite API token。当前历史读取要求与之前发布 review 相同的 GitHub PAT 账号；换账号前需迁移历史。历史或正文超过 GitHub 容量时会明确报错，不会截断 findings。

镜像默认使用 `623147552995.dkr.ecr.eu-west-1.amazonaws.com/code-review-agent-image:latest`，每次运行先执行 `docker pull`。可用 `REVIEW_IMAGE` 覆盖镜像地址，`REVIEW_PULL_IMAGE=false` 可用于本地已准备好的镜像；实际镜像 ID 和 digest 会保存在 `image.json`。

拉取和运行默认指定 `linux/amd64`，匹配当前 AWS agent。可用 `REVIEW_PLATFORM` 覆盖目标平台。ECR 镜像必须实际包含该架构；只加运行参数不会把 ARM 镜像转换为 amd64。遇到 `Exec format error` 时，在工具仓库运行修正后的 `bash auto/release`，重新构建并发布 amd64 的 `latest` 后再重试。

Review step 使用 `ecr#v2.3.0` 插件在执行脚本前登录 `eu-west-1` 的 ECR，沿用现有固件构建的插件版本。AWS 凭据和镜像拉取权限由 agent 环境提供。脚本通过 agent 内已有的 `shush` 调用 KMS 解密密文文件。`OCR_CONFIG_FILE` 也在 agent 侧读取，再仅向 Docker 传入配置环境变量，因此不用把 agent 私有文件路径挂到宿主机 Docker。

## Blocker 和 high 及以上问题的合并门禁

Buildkite YAML 默认设置 `REVIEW_GATE=true`。它自动启用历史复核，要求 `REVIEW_PASS=both` 和 `REVIEW_PUBLISH=true`，只使用实时 PR 的 base/head。门禁阈值固定为 finding 的 `high`，不包含普通 medium 问题，与控制审查力度的 `REVIEW_EFFORT` 不同。

- 当前和历史仍有效的 blocker 无论 severity 是 low、medium、high 还是 critical，都必须阻止合并，不受 high 阈值限制。普通 bug 中 high、critical 问题也阻止合并；未知严重度同样阻止。普通 low、medium 问题只作提醒。
- 历史问题必须逐条验证，不能因本轮漏报、降低严重度、GitHub 线程被手动 resolve 而放行。`open`、`unresolved` 保留；`fixed` 需要代码证据。经复核证明确属误报的 `dismissed`、当前明确排除的 `out_of_scope` 不再阻止，但不会标为已修复。
- 只修改 i18n/spec 等被规则明确排除的文件时，可按无待审文件完成；文件过大或无法解释的跳过仍不能通过。
- 两轮必须完整覆盖 PR。旧报告没有覆盖记录，或之前只跑过一轮时，重新做完整 PR 审查；有完整记录时才允许复核旧问题并增量审查。
- 模型失败、结果不完整、发布失败，或运行期间 PR head/base 改变，都不能通过。预检不发布门禁状态。
- 多个打开的 PR 共用同一个 head SHA 时，固定 commit status 无法区分各 PR。门禁在开始和结束时检查该情况，写入 error；开始时发现冲突会直接停止，不调用模型。为各 PR 使用独立的 head 提交，或关闭不再需要的 PR 后重新复核。检查覆盖全部打开的 PR，包括其他来源分支指向同一提交的情况。

状态名称固定为 `keystone-ai-review/merge-gate`，写到实际 PR head，手动构建的 `BUILDKITE_COMMIT` 即使是 master 也不影响它。PR webhook 构建先确认事件提交与当前 PR head 一致，再写 pending；重试过期构建不会改写新 head 的状态。完成且无未解决的 high 及以上问题、显式 blocker 或未知严重度问题时 success；有问题时 failure；运行失败时 error。读取历史和检查状态归属后，写最终状态前再次核对实时 head/base，避免使用已经失效的审查范围。结果记录在 `gate.json`，Buildkite 摘要区分“审查完成但被问题阻止”和“审查失败”。同一 PR 的 review jobs 串行执行，避免历史状态互相覆盖；不同 PR 可以并行。

The gate checks the saved head SHA, base SHA, and target branch strictly. A base update needs a new review even if the diff is unchanged. If the base changes before pending is posted, the job writes error on the same head and saves gate.json before it stops. A change during the start checks must not overwrite a newer gate status. A closed PR or a different head stops the old job without changing the new head's status.

These checks run while a review job is active. They do not watch a finished PR. After changing the target branch, start a new review with `@bot-keyst review fix` or a manual build; this change does not add an automatic base-change trigger.

**必须完成 GitHub 仓库配置才会真正阻止合并：**

1. 为现有 GitHub PAT 增加目标仓库的 **Commit statuses: Read and write**，保留 **Pull requests: Read and write**。若换了 token，按下面的 KMS 流程重新加密并提交。
2. 发布更新后的 review 镜像，把 CI 修改合入 `REVIEW_CI_BRANCH`。手动运行一个 PR，确认 GitHub 出现 `keystone-ai-review/merge-gate`。
3. 在 GitHub **Settings → Rules → Rulesets**（或 Branch protection）给 `master` 及其他允许合并 PR 的目标分支配置规则，启用 **Require a pull request before merging** 和 **Require status checks to pass**，添加 `keystone-ai-review/merge-gate`。要求分支保持最新；若规则要覆盖所有人，也需禁用管理员/角色绕过。

不要用 Buildkite 的通用 pipeline 状态替代这个 check。它可能属于手动构建的 master，或只代表事件过滤后跳过的 job。权限与 required check 说明见 [GitHub commit statuses](https://docs.github.com/en/rest/commits/statuses) 和 [protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)。本代码不会远程修改仓库规则。

触发方式保持 opened / ready 自动、后续 push 不跑。新提交没有该 head 的成功门禁，因此修复 push 后需在 PR 新发一条 `@bot-keyst review fix` 评论，或手动再运行 `REVIEW_PR=<编号>`；复核通过后才可合并。重试时保持同一个发布账号。

历史案例、单轮模型比较或只看报告时设置 `REVIEW_GATE=false`，不会发布门禁成功状态。直接在本地调用 shell 默认不启用门禁，只有 YAML 默认启用。

## Unclustered agent 的凭据配置

当前 agent 是 Unclustered，沿用基础设施已有的 AWS KMS + `shush` 方式，不使用 Buildkite 原生 `secrets:` 字段。现有加密脚本使用 `eu-west-1` 的 `alias/keystone`。本地身份需要该 key 的加密权限，agent 的 task role 需要解密权限；实际权限以 KMS 返回为准。

明文文件为 `ci/ai-review/secrets.env`，格式如下。保留字段名，把占位值替换为实际 key。保留 `GITHUB_TOKEN`，只需填写实际使用的模型 provider 对应的 key；不用的模型 key 可以省略：

```dotenv
OPENAI_APIKEY=xxxx
OPENROUTER_API_KEY=xxxx
GITHUB_TOKEN=xxxx
```

GitHub token 需要目标仓库的 Pull requests read/write 权限，用于读取 PR 和发布 review；启用门禁还需 Commit statuses read/write 权限，用于发布和读取 PR head 的状态。已有环境变量可覆盖它，优先级为 `GH_TOKEN`、`GITHUB_TOKEN`、密文文件中的 `GITHUB_TOKEN`。

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

密文必须由 `shush encrypt` 生成，不能直接换成 `aws kms encrypt` 的输出。把 `secrets.env.encrypted` 和 CI 代码一起提交并合入 `master`（默认的 `REVIEW_CI_BRANCH`）。Buildkite 从该分支读取脚本和密文；本地直接运行 `review-pr.sh` 时读取脚本同目录的密文。无需设置 `KMS_ENCRYPTED_OCR_CONFIG` 或 `KMS_ENCRYPTED_GITHUB_TOKEN`，也无需新增 agent hook。若已按旧方案配置 GitHub token 的 hook，移除该段及其密文变量；同时删除旧的明文 token 环境变量，才能使用文件中的新 token。仅轮换凭据无需重建 agent 或 review 镜像；首次启用内置 OpenRouter provider 时需要发布升级到 OCR 1.12.11 的 review 镜像。

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

脚本在读取 PR metadata 之前按需解密文件，每次运行最多解密一次。`GITHUB_TOKEN` 导出为 agent 侧的 `GH_TOKEN`，供读取 PR 和回写 review 使用；选中 provider 的模型 key 组装成 `OCR_CONFIG_JSON` 传给 review 容器，GitHub token 和其他 provider 的 key 不传入容器。脚本不把明文写入日志或宿主机产物。空行和 `#` 注释可用，值可以加成对的单引号或双引号；文件按文本解析，不进行 Shell 展开或命令执行。容器入口校验 JSON 并创建内部配置文件。`openai` 和 `openai-responses` 读取 `OPENAI_APIKEY`，`openrouter` 读取 `OPENROUTER_API_KEY`。自定义 endpoint 或其他 provider 可通过完整 OCR 配置传入。

使用 OpenRouter 时，在上述加密文件中加入 `OPENROUTER_API_KEY`，重新加密并提交。Buildkite 设置：

```text
REVIEW_PROVIDER=openrouter
REVIEW_MODEL=<OpenRouter model ID>
REVIEW_EFFORT=medium
REVIEW_PASS=both
REVIEW_MODE=chained
```

将模型占位值替换为 OpenRouter 中可用的完整模型 ID（含供应商前缀）。OCR 1.12.11 内置 `openrouter` provider，使用 OpenAI Chat Completions 协议和 `https://openrouter.ai/api/v1`；CI 生成 `providers.openrouter` 配置，不需要填写 `custom_providers` 或改审查规则。这里只选择模型，不新增模型推理强度参数。

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
5. 在同一页面的 **Additional Webhooks → Issue comments** 中开启 PR 评论构建，command word 只填 `@bot-keyst`，match mode 选 `contains`。该字段只保留第一个词，不能填整句；`exact` 要求整条评论等于 command word，因此无法匹配后面带 `review fix` 的评论。若需要在行内 finding 线程中回复命令，同时开启 **Pull request review comments** 并设置相同的 command word 和 match mode。这些入口使用 **Code** trigger mode。
6. 确认 GitHub 仓库发给 Buildkite 的 webhook 订阅 **Issue comments** 和 **Pull requests**；启用行内评论入口时还需 **Pull request review comments**。Buildkite 需要收到 PR 事件以获知分支和提交；已有 PR 若从未发送过该事件，可在配置好后 push 一次更新缓存，push 本身仍不运行审查。
7. 在 Buildkite GitHub 设置页面启用 **Filter builds using a conditional**，填写下面的完整条件，替换旧的仅允许 PR 事件的条件。Buildkite 将 opened 和 updated 放在同一个开关中，必须配合事件过滤才能保留新建 PR、排除后续 push。
8. 新建 PR，或将 draft PR 转为 ready，确认自动完成 review。随后向 PR 推送新提交，确认不再自动审查；再发表一条 `@bot-keyst review fix` 评论，确认进入复核并更新原 findings 和门禁。

```text
(
  build.source_event == "pull_request" &&
  (build.source_action == "opened" || build.source_action == "ready_for_review")
) ||
(
  (build.source_event == "issue_comment" || build.source_event == "pull_request_review_comment") &&
  build.source_action == "created"
)
```

PR 新建（包括 draft）和转为 ready 自动审查；push、synchronize、reopened 不运行审查。修复后在 PR 新评论中提及 `@bot-keyst`，并包含 `review` 和 `fix` 两个英文单词，例如：

```text
@bot-keyst review fix
```

Buildkite 先按 `contains` 匹配 `@bot-keyst`，并检查评论作者权限：仓库 owner、member、collaborator，或关联到有该 pipeline 构建权限的 Buildkite 用户。脚本使用 `BUILDKITE_GITHUB_COMMENT_ID` 重新读取评论，核对所属 PR 和 PR 的 open 状态；正文只要同时包含独立单词 `review` 和 `fix` 就接受，忽略大小写，不要求相邻或固定顺序，允许额外文字、多空格、换行和标点。缺少任一单词时记录为 `skipped`，不启动 review 容器、不发布审查或改写门禁；不会标为审查通过。编辑或删除评论不触发复核；重试需新发一条评论。

评论入口强制启用修复复核，即使设置了 `REVIEW_FIX=false` 或关闭了门禁，也会加载历史 findings。使用该 PR 当前 base/head，沿用现有模型、`medium`、`both`、`chained` 默认配置；没有历史结果时执行完整 PR 审查。运行记录保存到 `inputs/trigger.json`。评论读取沿用现有 GitHub token 的 Pull requests 权限，无需新增 token。

同一提交可通过新评论再运行，因此 **Skip when pull request has existing build for commit and branch** 必须保持关闭。同一 PR 的复核串行执行。若排队期间 PR head 改变，脚本会提示重新触发；旧结果不会为新提交放行。

手动入口仍可使用：在 Buildkite 选择 **New Build**，branch 设为 `master`、commit 设为 `HEAD`，为这次 build 设置 `REVIEW_PR=108`（替换成目标 PR 编号）和 `REVIEW_FIX=true`。

这些事件由 Buildkite 已有的 GitHub webhook 处理，无需添加 GitHub Actions workflow，也无需在 GitHub 中新增 Buildkite API token。页面条件和原生评论命令匹配在创建 webhook build 前过滤事件；YAML 也限制 review step 的事件。`review-pr.sh` 在读取凭据和启动模型之前再检查事件，让仍使用旧 YAML 的 PR 也能跳过 push。只更新代码而未保存页面条件时，push 仍可能产生一个跳过审查的 build，而评论命令仍会被旧条件过滤。手动构建不受 webhook 事件过滤影响。

同一 head 转为 ready 或手动新建 build 时可以再次审查并发布。只有同一个 build 对同一 PR/head 的重复发布会被跳过。

仅修改仓库文件不会更新 Buildkite 页面中保存的 bootstrap command 或 GitHub 开关；需要在页面保存一次。之后每次运行自动获取 `REVIEW_CI_BRANCH` 的最新 CI 文件。新增评论触发仅改动固件仓库的 CI 文件，已部署修复复核镜像时无需再次重建镜像。触发机制见 [Buildkite GitHub 文档](https://buildkite.com/docs/pipelines/source-control/github#running-builds-on-additional-github-events)。

YAML 为自动触发提供以下默认配置：`openai-responses`、`gpt-6.1-sol`、`medium`、`both`、`chained`，并开启回写和门禁。第二轮 blocker 审查接收第一轮 bug findings；如果加入 findings 后 background 超限，就省略 findings。通过 Buildkite 手动创建 build 时，可以传入同名变量覆盖这些值；pipeline upload 会把最终值写入 review step。

完整审查成功后，脚本使用 `COMMENT` 类型提交一个 GitHub review，不会 approve 或 request changes。能确认属于当前 diff 的位置会成为行内评论，其余 findings 会放在 review body。正文包含实际 provider、model 和本次所有审查轮次的输入、输出、总 token 数；缓存用量单独展示，不重复加到总数。模型信息读取 `review/run.json`，token 数直接读取汇总 `review/report.json` 的 `tokens`，缺失时显示 `not recorded`，不用当前环境变量猜测，也不把缺失记成 0。行内评论被 GitHub 拒绝而退回正文发布时，仍保留这些信息。

发布前会重新读取 PR：如果 PR 已关闭或 head 在模型运行期间发生变化，就跳过旧结果。去重匹配 repo、PR、head marker 和完整 `BUILDKITE_BUILD_URL`：同一个 build 重试不会重复发布，新的 build 可以在相同 head 上发布新的审查结果，适用于修复后复审和换模型复测。已有旧 review 中的 Buildkite 链接同样参与匹配。直接在本地发布、未设置 `BUILDKITE_BUILD_URL` 时，保留按 PR/head 去重；从历史 build 的产物补发时，应设置原 build 的 URL。

发布结果及模型/token 信息写入 `publish.json`。Buildkite annotation 同时显示发布状态、review 链接和跳过原因。

是否强制阻止合并由 GitHub 的 required check 配置决定。仅发布 failure 而未启用仓库规则，不会禁用合并按钮。

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

Buildkite reports whether the review job completed. A completed review and publication return exit code 0, even when findings block merging. In that case, status.json records status=complete and gate=failure; the separate GitHub merge gate still blocks the PR. Model, script, publication, and gate update errors return a nonzero exit code. Reports do not include timing, plan, or token analysis.

## 当前边界

2026-10-08：代码支持历史问题复核、PR 评论命令和独立合并状态；门禁需部署新镜像/CI，并配置 GitHub required check 后生效。评论触发需保存 Buildkite 和 GitHub webhook 配置；opened / ready_for_review 自动审查保留，push 不运行审查。门禁覆盖同一发布账号的 OCR 历史 findings，不自动导入其他机器人或人工评论。
