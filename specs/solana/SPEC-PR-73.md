# PR #73：Solana 交易审查与签名加固规范

- PR：https://github.com/KeystoneHQ/keystone3-firmware-internal/pull/73
- 核对基线：origin/master `bdc6034ec`；PR 本地 HEAD `85051bee9`。
- 本文与 fingerprint 回归测试、Jupiter 分支说明一同提交；远端 PR 需推送后才包含这些补充。
- 本文用于说明行为契约和验证入口，不替代代码、测试及官方协议规范。实现与本文不一致时，应指出差异并验证影响，不应据本文忽略问题。

## 1. 目标与范围

在既有 Legacy、V0、V1 交易支持上，加强请求类型、派生路径、签名人和指令解析校验；让用户能够核对完整指令及未解析账户，并在 Details 中查看可还原 wire message 的 JSON。

本 PR 不是重新引入 SOL V1：V1 基础支持及 Details 文本分页已在基线中。这里增加安全校验、原始 message JSON、结构化 Advanced 展示和对应测试。另包含 release 子仓库指针更新，用于移除 legacy OTA 发布流程，属于独立发布变更，不应归入 SOL 协议逻辑。

## 2. 请求类型与签名边界

### 2.1 Transaction 请求

- 扫描检查与最终签名前均通过 `validated_payload` 验证。
- `SignType::Transaction` 必须通过完整交易解析及所支持指令的语义解析；失败直接返回错误，不自动降级为消息签名。
- 扫码视图路由按 Transaction 请求进入交易检查，不能因交易解析失败而转成普通消息页面。
- 最终签名从 seed 和请求路径派生公钥，验证其属于交易 required signers。
- 签名对象是请求中的原始 sign_data，不是展示 JSON 或重建后的交易。

实现入口：`rust/rust_c/src/solana/mod.rs` 的 `validated_payload`、`solana_check`、`build_sign_result`；`rust/rust_c/src/common/ur_ext.rs` 的 SOL 视图推断。

### 2.2 Message 请求：当前实现及未关闭边界

`classify_payload` 当前依次判断：

1. `Message::read_exact` 成功：Transaction。
2. 完整解析失败，但首字节为 `0x81`：MalformedTransaction。
3. 仍具有可解析的交易前缀：MalformedTransaction。
4. 其余数据：Message。

Message 请求仅接受最后一种结果。因此，完整交易改标为 Message、可识别交易前缀附加尾随数据，以及损坏的 V1，都不能通过消息签名路径。

**当前不能声称所有畸形交易均被识别：** 截断 V0、未知版本等数据可能落入第四种结果。该边界尚待明确兼容策略和补充测试；不应在 review 中将其视为已修复。

不能仅以首字节最高位为 1 就拒绝消息：UTF-8 文本可能以高位字节开头，标准 offchain 消息也使用 `0xff` 起始。现有实现识别 offchain v0 正文，不代表完整支持所有 offchain 版本。修复分类时应同时验证普通 UTF-8、合法 offchain 格式以及交易伪装，不应只检查一个前缀。

实现入口：`rust/apps/solana/src/lib.rs` 的 `classify_payload`、`parse_offchain_message_v0`、`parse_message`。

## 3. 路径、fingerprint 与 signer

接受路径形态为：

- `m/44'/501'`
- `m/44'/501'/account'`，account 为 0～49
- `m/44'/501'/account'/0'`

允许省略开头的 `m/` 后规范化；所有路径分量必须 hardened。额外层级、错误 coin type、超范围 account、非零 change 均拒绝。

`solana_check` 要求设备 master fingerprint 长度为 4，并与请求路径携带的 source fingerprint 匹配；缺失或不匹配拒绝。fingerprint 是请求归属检查，不能替代最终签名时的 required signer 校验。

实现入口：`validate_derivation_path`、`solana_check`、`build_sign_result`、`Message::validate_signer`。

本次补充：对有效 fixture 仅翻转设备 fingerprint 的一个 bit，断言返回具体的 `MasterFingerprintMismatch`，而不是仅断言“任意失败”。

## 4. 指令解析与 raw review 的边界

| 输入类别 | 行为 |
| --- | --- |
| 已支持指令，格式及字段合法 | 解析语义并展示 |
| 已支持指令，存在非法截断、长度或尾随数据 | 返回错误，不降级 Unknown |
| 不认识的 program ID | 保留原始数据进入 Unknown/raw review |
| Jupiter v6 完整但不支持的 discriminator | 保留原始数据进入 Unknown/raw review |
| Jupiter discriminator 少于 8 字节 | InvalidData，拒绝 |
| 已支持 Jupiter discriminator，但内容解析失败 | ProgramError，拒绝 |

“已知程序”不等于“固件支持该程序的全部指令”。对 Jupiter，未支持的完整 discriminator 与已支持指令解码失败，必须区别处理。

当前错误类型 `UnsupportedProgram` 同时用于未知 program ID 和未支持的 Jupiter 指令；名称不能替代控制流判断。若建议将后者全部拒绝，应明确这是取消相应 raw review 能力的行为变更。

报告降级漏洞时，应给出具体输入和到达路径，说明它是未支持指令还是已支持指令的损坏数据，以及最终进入哪种 UI。该要求不意味着未知指令天然安全；其风险必须通过原始数据展示而非可信业务解释来表达。

实现入口：`instruction.rs` 的 `Instruction::parse`；`message.rs` 的 `to_program_details`。

测试入口：`distinguishes_malformed_and_unknown_jupiter_instructions`，以及 System、SPL、Token Swap、Lending、Jupiter 的截断和尾随数据测试。

## 5. V1 TransactionConfig 与 JSON 不变量

官方依据：[SIMD-0385 Transaction V1 / TransactionConfigMask](https://github.com/solana-foundation/solana-improvement-documents/blob/main/proposals/0385-transaction-v1.md#transactionconfigmask-1)。

当前 mask 只允许 bit 0～4；priority fee 占用低两位，这两位只能同时为 0 或同时为 1。

解析入口有明确校验：

```rust
if mask & !0x1f != 0 || matches!(mask & 3, 1 | 2) {
    return Err(invalid("unsupported V1 transaction config mask"));
}
```

这里 `1 | 2` 是 Rust 模式“1 或 2”，不是按位或运算。

Details JSON 的 `config_values` 保存 wire 上的 `u32` words，而不是把每个配置项变成一个 JSON 元素：

- fee 不存在：0 个 word。
- fee 存在：2 个 word，低 32 位在前、高 32 位在后。
- 其他每个配置 bit：1 个 word。
- 未携带的字段不向原始 JSON 中补默认值。

因此，对合法 mask，`config_values.len() == mask.count_ones()`。

| mask | word 数 | 处理 |
| --- | ---: | --- |
| `00000` | 0 | 接受 |
| `00011` | 2 | 接受 |
| `00111` | 3 | 接受 |
| `11111` | 5 | 接受 |
| 低两位为 `01` 或 `10` | 不适用 | 解析入口拒绝 |

测试 `every_config_mask_and_full_u64_fee` 虽遍历 `0..32`，但显式跳过非法低两位组合。JSON 往返断言不承担接受或修复非法 mask 的职责。

V1 ComputeBudget 指令按 V1 规则作为 no-op 处理，实际配置来自 TransactionConfig。其存在不能被误解释为覆盖配置，也不应凭 Legacy/V0 的 ComputeBudget 解析要求拒绝合法 V1。

实现入口：`message_v1.rs`、`message_json.rs::assert_json_roundtrip`、`message.rs` 的 V1 ComputeBudget 分支。

## 6. 用户审查页面

### Overview

- 金额通过整数计算格式化，避免浮点精度损失。
- 已识别与未知指令按原始顺序披露，不能因存在可信主操作而隐藏其他指令。
- TransactionConfig 是元数据，不伪造为真实链上指令。
- 对单条 SystemTransfer，若发送者是静态地址、收款者是 ALT 引用，可保留金额和动作展示，但收款者必须标为未解析。
- 其他当前无法安全解释的 ALT 布局进入通用 Instructions 展示，不能假装已经解析真实账户。

ALT 转账卡片顺序：警告 → Value/Main Action → From → Unresolved Destination。`lastView` 始终表示已定位的最后一个卡片；新卡片定位后再更新它。

fixture：`Solana Transaction Security / 11 / 由 ALT 提供的 SOL 收款地址`。本地播放器该 fixture 的旧说明仍写“必须进入通用 Instructions”，已不符合本分支，应以实际解析规则和回归断言为准。

### Advanced

Tab 顺序为 Overview → Advanced → Details。Advanced 使用结构化卡片和分页，展示：message 版本、required signatures 数量、fee payer、可用时的 device signer、账户索引与权限、指令 program/accounts 索引及数据长度、recent blockhash 或 lifetime specifier。

ALT 只展示表地址及索引，不推测真实地址。合并账户顺序为静态账户、所有 lookup 的 writable 账户、所有 lookup 的 readonly 账户。

TransactionConfig 已在 Overview 展示，不在 Advanced 重复。

### Details

常规交易 Details 展示原始 message 结构 JSON，而非内部业务解析 JSON。保留 header、地址、指令索引、原始 instruction data、ALT 引用以及 V1 特有字段。原始指令数据使用 base58 并显式标记编码。

JSON 可以独立重建原始 message bytes，并通过往返测试核对。这里不包含 message 外部的完整交易签名数组，也不是 RPC 的 parsed transaction JSON。现有 Squads 专用展示分支需单独评估，不能把一般 Details 行为推定到所有专用页面。

实现入口：`parser/mod.rs`、`advanced.rs`、`message_json.rs`、`rust_c/src/solana/structs.rs`、`gui_sol.c`。

## 7. 测试与验证边界

自包含数据：`rust/rust_c/tests/fixtures/solana-security.json` 使用公开 abandon 测试助记词对应的 UR 数据，仅供测试；CI 不依赖开发者本地 fixtures 仓库。

主要回归入口：

- `canonical_fixtures_check_and_sign`：扫描检查、最终签名、请求类型不匹配；本次新增 fingerprint 负例。
- `v1_security_checks_apply_at_final_signing`：V1 fixture 在最终签名入口的校验及改标 Message 的拒绝。
- `canonical_display_regressions_preserve_order_alt_and_message_length`：指令顺序、ALT 展示与消息长度。
- `solana_transaction_fixtures_never_fall_back_to_message_view`：UR 视图路由。
- `assert_json_roundtrip`：JSON 独立还原原始 message bytes。

建议复现命令（从 `rust/` 目录运行）：

```sh
cargo test -p app_solana --locked
cargo test -p rust_c --offline --no-default-features --features simulator-multi-coins solana --lib
```

本轮实际验证：针对 `solana::fixture_tests` 的 3 项测试通过，包含本次 fingerprint 补充；`git diff --check` 通过。该结果不等于本轮重新完成全部 Rust、ARM 或硬件验证。

新增 UI 翻译修改 `data.csv` 后同步生成翻译表与所需字体；当前相关 C 文件做过语法检查，但多语言布局仍需视觉验证。

后续 review 修复（当前工作区）：native 指令反序列化使用调用方指定的编译期 1232 字节 bincode limit，并检查输入长度及完整消费；Vote slots/lockouts 还在集合分配前校验声明数量，避免 Serde 根据过大的 size_hint 预分配；测试覆盖超大声明长度、边界和正常 wire 数据。Advanced 内部文案使用 `{{solana_advanced_*}}` 翻译标记，C 展示层按当前语言展开，原始 Details JSON 不做翻译。模拟器 `ui_simulator/assets/page_sol.json` 从固件内嵌布局提取并纳入版本控制。

## 8. Review 应重点核实的开放问题

1. Message 分类边界：哪些截断/未知版本输入可签为消息，哪些可能造成实际交易签名风险；怎样保留合法 UTF-8 和 offchain 消息。
2. Raw review 是否确实披露所有 program、账户索引和指令数据，是否仍存在错误可信化的专用页面。
3. 新增长字符串、ALT 内容、分页和多语言是否存在裁剪或重叠。
4. UI 检查与最终签名入口是否保持同等安全约束；不得仅凭扫描成功断言签名安全。
5. 任何“允许未知指令”“允许任意消息”的兼容策略都应明确展示边界，不应被本文解释为无限制放行的理由。

本轮补充验证：app_solana 87 项测试及 SOL FFI 4 项测试通过；Advanced 翻译展开的 7 个独立 C 用例通过 ASan/UBSan 检查，模拟器 JSON 与固件内嵌 JSON 相同。未执行真机或模拟器视觉验证。

最新 review 修复（工作区）：Squads 聚合动作之间夹有非聚合指令时，回退到按原始顺序的通用 Instructions 展示；连续聚合组仍保留专用 UI。已补端到端解析回归，核对 program 和原始 data 顺序。PR 涉及的 C 文件中写内存及格式化操作统一使用有界安全接口，传入实际目标或剩余容量；全仓存量扫描另存本地 .agents 清单。
