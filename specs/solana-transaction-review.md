# Solana 交易审查与签名加固

## 背景

固件已有 Legacy、V0、V1 交易支持和 Details 分页。交易请求来自不可信主机或二维码，主机声明的 `SignType` 只是提示，可能与载荷内容不符（dApp 会把 Sign-In With Solana 消息标成 Transaction）；因此需要以载荷内容判定请求类型，同时避免已支持指令解析失败后降级，以及聚合页面遗漏实际签名动作。

本文描述当前行为契约与验证入口，供实现、QA 和 review 对照。实现与规范冲突时应报告差异，不能用本文代替代码验证。关联 PR：#73。

## 目标

- 扫描、解析、视图路由和最终签名采用一致的**内容分类**（`classify_payload`）结果；主机声明的 `SignType` 不参与判定。
- 检查路径、钱包归属与 required signer，签名原始请求字节。
- 完整披露指令顺序、未知程序、未解析账户与 V1 配置。
- 提供结构化 Advanced 和可还原原始 message 的 Details JSON。

## 非目标

- 不重新引入已存在的 V1 基础支持，不改写 Solana 网络协议。
- 不联网解析 ALT、推测 cluster，或把未支持指令解释成可信业务动作。
- 不取消任意消息签名兼容性；截断及未知版本消息分类仍有开放边界。
- 不把 release 子仓库的 legacy OTA 发布调整归入 Solana 协议逻辑。

## 请求分类

### Transaction 请求

- 扫描检查、两个交易解析入口（`solana_parse_tx` / `solana_parse_tx_with_pubkey`）及最终签名前均通过 `validated_payload` 验证；交易解析入口仅接受 Transaction 结果。
- 内容分类为 Transaction 时必须通过完整交易解析及所支持指令的语义解析；失败直接返回错误，不自动降级为消息签名。
- 主机声明 `SignType::Transaction` 不构成约束：载荷内容分类为 Message 时按消息处理（`validated_payload` 返回 Message，最终签名走消息路径），只有 MalformedTransaction 被拒绝。
- 扫码视图路由按内容分类决定：Transaction / MalformedTransaction 进入交易检查页，Message 进入消息页；不因 `SignType` 声明改变页面。
- 最终签名从 seed 和请求路径派生公钥，验证其属于交易 required signers。
- 签名对象是请求中的原始 sign_data，不是展示 JSON 或重建后的交易。

实现入口：`rust/rust_c/src/solana/mod.rs` 的 `validated_payload`、`solana_check`、`build_sign_result`；`rust/rust_c/src/common/ur_ext.rs` 的 SOL 视图推断。

### Message 请求：当前实现及未关闭边界

`classify_payload` 当前依次判断：

1. `Message::read_exact` 成功：Transaction。
2. 完整解析失败，但首字节为 `0x81`：MalformedTransaction。
3. 仍具有可解析的交易前缀：MalformedTransaction。
4. 其余数据：Message。

消息路径仅接受最后一种结果（按内容分类，不看 `SignType` 声明）。因此完整交易、可识别交易前缀附加尾随数据，以及损坏的 V1，都不能通过消息签名路径，即使请求声明为 Message。反向同理：声明为 Transaction 但内容是普通文本（如 SIWS）会被识别为消息并进入消息签名，不再以“类型不符”拒绝。

**当前不能声称所有畸形交易均被识别：** 截断 V0、未知版本等数据可能落入第四种结果。该边界尚待明确兼容策略和补充测试；不应在 review 中将其视为已修复。

不能仅以首字节最高位为 1 就拒绝消息：UTF-8 文本可能以高位字节开头，标准 offchain 消息也使用 `0xff` 起始。现有实现识别 offchain v0 正文，不代表完整支持所有 offchain 版本。修复分类时应同时验证普通 UTF-8、合法 offchain 格式以及交易伪装，不应只检查一个前缀。

实现入口：`rust/apps/solana/src/lib.rs` 的 `classify_payload`、`parse_offchain_message_v0`、`parse_message`。

### 类型提示与内容分类的取舍

2026-09-14 的加固（`58a3e207a`）曾把 `SignType` 放在内容判断之前，类型不符即拒绝，并新增 `transaction-security--07/08` 两个“消息被标成交易”的固件用例。实际生态中 dApp（如 Magic Eden）会把 Sign-In With Solana 消息放进 `SignType::Transaction` 请求，这类请求在扫描阶段被当作交易解析而失败（`SolanaInvalidData`/603，UI 只显示通用错误页）。

2026-09-24 恢复内容优先（即 `756df0e73` 的行为）：

- `validated_payload` 与 `ur_ext::infer` 均以 `classify_payload` 结果为准；`SignType` 不再参与判定。
- MalformedTransaction 仍被拒绝，交易不会以不透明字节进入消息签名路径。
- 代价：不再强制请求方声明正确，类型提示也不再影响结果 —— 判据只有载荷本身。解析不成完整交易、也不带交易前缀的字节（`classify_payload`）一律按消息签名，展示形态由 `parse_message` 决定：可打印、非 CJK、不含 NUL 的 UTF-8（例如 Magic Eden 的 SIWS 正文、`transaction-security--08` 这类短文本）进入 `utf8_message`，按普通消息页展示，**不带** `solana_blind_sign_title`，也**不显示** `solana_unparsed_message_warning`；只有 CJK、含 NUL 或非 UTF-8 的载荷才进入 `raw_message`（十六进制），才带盲签标题与该提示（`gui_analyze.c` 按模板分支传 `raw`，拼提示见 `gui_sol.c` 的 `GuiShowSolMessagePaged`）。因此“未解析消息必须提示”目前只覆盖 raw 分支；若产品要求给不带 offchain 封装的文本加提示，或收紧为只放行 SIWS/offchain 结构，应在此处补充并同步用例。

## 修改方式

### 路径、钱包归属与签名人

接受任意合法 hardened 路径（不固定 purpose 或 coin type 前缀），不限制账户索引为前 50 个，也不限制 change 为 0。路径最多包含 5 个派生分量（`m` 不计入深度），规范化后的 ASCII 路径总长度最多为 61 字节；长度检查在分配规范化字符串之前执行。允许省略开头的 `m/` 后规范化。所有分量必须为 0～2147483647 的 hardened 索引；非 hardened、空分量及溢出索引均拒绝。

预存公钥表仅用于常见路径的公钥缓存。缓存路径在扫码检查阶段校验设备公钥属于 required signers，不匹配则拒绝并提示无效二维码。自定义路径使用完整交易解析进入审阅，最终签名仍从实际 seed 按请求路径派生公钥并校验 required signer；自定义路径的消息不展示未经派生确认的 From 地址。交易 Details 和消息审阅页面展示完整请求路径，长路径可滚动查看。

`solana_check` 要求设备 master fingerprint 指针非空且长度为 4，并与请求路径携带的 source fingerprint 匹配；缺失或不匹配拒绝。fingerprint 是请求归属检查，不能替代最终签名时的 required signer 校验。

实现入口：`validate_derivation_path`、`solana_check`、`build_sign_result`、`Message::validate_signer`。


最终签名还从实际签名 seed 计算 BIP32 master fingerprint，与请求的 source fingerprint 比较；缺失或不匹配即拒绝。此校验对交易和普通消息均生效，不依赖扫描阶段曾经成功。

### 指令解析与原始展示

| 输入类别 | 行为 |
| --- | --- |
| 已支持指令，格式及字段合法 | 解析语义并展示 |
| 已支持指令，存在非法截断、长度或尾随数据（ComputeBudget 完整字段后的尾随字节除外） | 返回错误，不降级 Unknown |
| 不认识的 program ID | 保留原始数据进入 Unknown/raw review |
| Jupiter v6 完整但不支持的 discriminator | 保留原始数据进入 Unknown/raw review |
| Jupiter discriminator 少于 8 字节 | InvalidData，拒绝 |
| 已支持 Jupiter discriminator，但内容解析失败 | ProgramError，拒绝 |

“已知程序”不等于“固件支持该程序的全部指令”。对 Jupiter，未支持的完整 discriminator 与已支持指令解码失败，必须区别处理。

当前错误类型 `UnsupportedProgram` 同时用于未知 program ID 和未支持的 Jupiter 指令；名称不能替代控制流判断。若建议将后者全部拒绝，应明确这是取消相应 raw review 能力的行为变更。

报告降级漏洞时，应给出具体输入和到达路径，说明它是未支持指令还是已支持指令的损坏数据，以及最终进入哪种 UI。该要求不意味着未知指令天然安全；其风险必须通过原始数据展示而非可信业务解释来表达。

实现入口：`instruction.rs` 的 `Instruction::parse`；`message.rs` 的 `to_program_details`。

测试入口：`distinguishes_malformed_and_unknown_jupiter_instructions`，以及 System、SPL、Token Swap、Lending、Jupiter 的截断和尾随数据测试。

原生指令反序列化使用调用方指定的编译期 1232 字节 bincode limit，并检查输入长度及完整消费。Vote slots / lockouts 在集合分配前检查声明数量，防止按过大的 size_hint 预分配。

### V1 配置与 JSON

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

## 用户审查页面

### Overview

- SOL / SPL 转账金额通过整数计算格式化。旧 Squads 创建聚合代码仍有浮点计算，但当前解析入口已不再选择该创建布局，不能声称全仓浮点代码已移除。
- 已识别与未知指令按原始顺序披露，不能因存在可信主操作而隐藏其他指令。
- TransactionConfig 是元数据，编号固定为 0，不伪造为真实链上指令。原始指令页同样保留此配置，顺序为风险提示 → TransactionConfig 卡片 → 原始指令；没有配置的 Legacy/V0 不补造 V1 配置。
- 对单条 SystemTransfer，若发送者是静态地址、收款者是 ALT 引用，可保留金额和动作展示，但收款者必须标为未解析。
- 其他当前无法安全解释的 ALT 布局进入通用 Instructions 展示，不能假装已经解析真实账户。

ALT 转账卡片顺序：警告 → Value/Main Action → From → Unresolved Destination。`lastView` 始终表示已定位的最后一个卡片；新卡片定位后再更新它。

回归 fixture：`Solana Transaction Security / 11 / 由 ALT 提供的 SOL 收款地址`。

### Advanced

Tab 顺序为 Overview → Advanced → Details。Advanced 使用结构化卡片和分页，展示：message 版本、required signatures 数量、fee payer、可用时的 device signer、账户索引与权限、指令 program/accounts 索引及数据长度、recent blockhash 或 lifetime specifier。

ALT 只展示表地址及索引，不推测真实地址。合并账户顺序为静态账户、所有 lookup 的 writable 账户、所有 lookup 的 readonly 账户。

TransactionConfig 已在 Overview 展示，不在 Advanced 重复。

### Details

常规交易 Details 展示原始 message 结构 JSON，而非内部业务解析 JSON。保留 header、地址、指令索引、原始 instruction data、ALT 引用以及 V1 特有字段。原始指令数据使用 base58 并显式标记编码。

JSON 可以独立重建原始 message bytes，并通过往返测试核对。这里不包含 message 外部的完整交易签名数组，也不是 RPC 的 parsed transaction JSON。当前 Squads 创建交易已回退原始指令页；保留的专用页面仍需核对实际 UI 路径。

实现入口：`parser/mod.rs`、`advanced.rs`、`message_json.rs`、`rust_c/src/solana/structs.rs`、`gui_sol.c`。

### Squads 布局选择

- 包含 MultisigCreate / MultisigCreateV2，或 Squads 交易附带 System Transfer：使用按原始顺序的 Instructions 展示。0、1、2、3 笔 transfer 均不得依赖旧 C 布局的固定索引。
- 普通提案保留专用布局；聚合动作被其他指令隔开时，回退原始指令页，不能把后面的动作提前展示。
- 原始指令页通过 `additional_overviews` 单独携带 TransactionConfig，C 使用已有 general 卡片渲染；配置不进入原始指令数组，也不在 Advanced 重复。

### 内存、翻译及分页

- 交易和消息解析失败时，在退出前释放对应 `TransactionParseResult`；成功结果交由现有页面清理流程释放。
- PR 涉及的 C 内存复制、清零及格式化使用安全接口，传入真实目标容量或剩余容量。
- Advanced 每页最多四个条目，内容容器可纵向滚动。`lv_obj_create` 默认开启 SCROLLABLE；清除父容器滚动标志不清除子容器标志。
- Advanced 内部 `{{solana_advanced_*}}` 标记仅由 C UI 翻译展开，原始 Details JSON 不翻译。
- 翻译修改后运行现有生成流程，提交发生变化的翻译表和字体。字符已覆盖时可以没有字体差异，不能仅凭某次提交未修改字体判断缺字。


## 场景与预期

| 场景 | 预期结果 |
| --- | --- |
| 完整交易标记为 Message | 解析及最终签名拒绝 |
| Transaction 请求解析失败 | 拒绝，不转消息页面 |
| fingerprint 缺失或不匹配 | 扫描及最终签名拒绝 |
| fingerprint 指针为空或长度不为 4 | FFI 返回错误，不构造非法切片 |
| 派生公钥不在 required signers | 最终交易签名拒绝 |
| 支持的指令截断或包含尾随字节 | 语义解析拒绝；ComputeBudget 完整字段后的尾随字节按验证器行为忽略 |
| 未知程序或完整但未支持的 Jupiter discriminator | 保留原始数据供审查 |
| Squads 创建或附带转账 | 按原始顺序展示全部指令 |
| V1 回退原始指令页 | 保留默认或显式 TransactionConfig，不占指令编号 |
| 交易或消息解析失败 | 错误结果释放，不积累堆泄漏 |

## 核心不变量

- 最终签名字节始终是请求 sign_data，不是展示 JSON 或重建数据。
- fingerprint 校验不能替代 required signer 校验；两者分别验证请求归属和交易签名角色。
- 已支持指令解码失败与未知指令是不同状态，不得共用降级路径。
- 原始展示必须保留全部指令顺序和字节，不能丢失 message 级配置。
- JSON 中 config_values 保留 wire words；合法 mask 才满足 count_ones 往返断言。
- 不把未解析 ALT 引用显示成已确认地址，不从 message 推断主网或测试网。

## 涉及文件

| 文件 | 职责 |
| --- | --- |
| `rust/apps/solana/src/lib.rs` | payload 分类、离线消息与路径校验 |
| `rust/apps/solana/src/message.rs`、`message_v1.rs` | wire 解析、账户与 V1 配置 |
| `rust/apps/solana/src/parser/mod.rs` | 布局选择、顺序、配置卡片 |
| `rust/apps/solana/src/message_json.rs`、`advanced.rs` | Details JSON 与 Advanced 数据 |
| `rust/rust_c/src/solana/mod.rs`、`structs.rs` | FFI 校验、最终签名和展示数据转换 |
| `rust/rust_c/src/common/ur_ext.rs` | SOL 请求页面路由 |
| `src/ui/gui_chain/multi/web3/gui_sol.c` | UI、翻译展开、分页及结果释放 |
| `rust/rust_c/tests/fixtures/solana-security.json` | 自包含 UR 回归数据 |

## Review 重点

- Message 分类仍存在未关闭边界：截断 V0 和未知版本可能归类为普通消息。报告应区分可签任意字节与实际可执行交易签名风险，并验证 UTF-8 / offchain 兼容性。
- 核对最终入口是否独立验证 fingerprint、路径和 signer，而非仅依赖扫描阶段。
- 核对 Squads 回退是否保留每条指令，以及 raw Overview 是否带上 V1 配置。
- 路径检查需查看完整 C 表范围：ROOT 项位于 BIP44_0 到 BIP44_CHANGE_49 之间，不因起始枚举名称就认定根路径遗漏。
- 核对分页、长地址和翻译后的布局；字体缺失需要具体缺失字形证据。
- 未知指令和任意消息的兼容策略不代表它们天然安全；应明确展示能力及开放风险。

## 验证

从 `rust/` 运行：

```sh
cargo test -p app_solana --locked
cargo test -p rust_c --offline --no-default-features --features simulator-multi-coins solana --lib
```

关键回归：

- `canonical_fixtures_check_and_sign`：有效请求、错误类型、缺失/错误 fingerprint、空指针与最终签名；并断言两种 `SignType` 提示下分类与签名结果完全一致。其中 `transaction-security--07/08` 是“消息被标成交易”的用例，期望 success。
- `v1_security_checks_apply_at_final_signing`：V1 最终签名，以及两个交易解析入口在两种类型提示下结果一致。
- `canonical_display_regressions_preserve_order_alt_and_message_length`：顺序、ALT 与消息长度。
- `squads_creation_and_transfers_use_complete_raw_review`：两种创建指令及多笔转账完整披露。
- `raw_squads_review_preserves_message_config`：默认/显式 V1 配置及原始指令、JSON 往返。
- `solana_view_type_follows_the_payload_not_the_type_hint`：UR 路由按内容分类，且两种提示下视图类型一致。
- `message_payload_with_transaction_hint_follows_the_payload`：SIWS 文本加 Transaction 提示时判定为消息并签名成功（Magic Eden 场景）。
- `every_config_mask_and_full_u64_fee`、`assert_json_roundtrip`：合法 mask、完整 u64 与 JSON 还原。

fixtures 使用公开 abandon 测试助记词，仅用于测试；CI 不依赖开发者本地 fixtures 仓库。

最近一次代码验证（2026-09-20，rebase origin/master 后）：90 项 app_solana 测试、4 项 SOL FFI 测试通过；C 语法及差异检查通过。翻译与字体重新生成无额外差异。未完成本轮真机或模拟器视觉验证，不据此声称 UI 已全面验证。

2026-09-24 内容优先调整后：`cargo test -p rust_c --no-default-features --features simulator-multi-coins --lib` 86 项通过（含上述 SOL 用例）；`cargo fmt -p rust_c -- --check` 干净；WEB3 模拟器重建、链接与签名通过。仍未做真机或模拟器视觉验证。

### 原始指令概览布局

未解析交易的账户列表每页展示最多 3 个账户，页内支持触摸滚动，长内容显示滚动条；ALT 账户保留 Table、Index 和说明入口。Data 超过 512 字节时使用分页组件；单页内容直接以自动换行、内容高度标签展示，不显示翻页控件。限制单张指令卡片高度，避免大量账户和长 Data 超出 LVGL 坐标范围后与后续字段或卡片重叠。

### ComputeBudget 尾随字节兼容

Legacy/V0 ComputeBudget 指令按验证器的 `try_from_slice_unchecked` 行为读取完整字段并忽略尾随字节，仍拒绝截断字段及未知类型。原始消息字节保持不变用于签名。例如 `010000010000000000` 解析为 RequestHeapFrame 65536 bytes。V1 的 ComputeBudget no-op 规则不变。

### Squads V4 配置交易

支持 `config_transaction_create`（discriminator `9bec57e4894b5127`），按 Squads 官方 `ConfigTransactionCreateArgs` / `ConfigAction` 的 Borsh 字段顺序解析 actions 与 memo。支持 AddMember、RemoveMember、ChangeThreshold、SetTimeLock、AddSpendingLimit、RemoveSpendingLimit、SetRentCollector；账户地址以 base58 展示。拒绝字段截断、未知 action、空 actions 和尾随数据；至少要求官方定义的五个账户。

包含配置创建指令时使用按原始指令顺序排列的 General 审阅，展示 multisig、creator、全部配置操作和 memo，避免专用 Proposal 卡片省略配置变更。签名仍使用原始消息字节。此为离线解析支持，不替代链上账户状态和权限校验。

历史兼容原因：`58a3e207a` 之前 `Message::to_program_details` 将所有解析错误降级为 Unknown/raw；加固后只允许 UnsupportedProgram 降级，Squads 未支持指令的 Custom(1) 因而拒绝。此处通过补充语义解析解决，未恢复所有错误降级。

协议来源：https://github.com/Squads-Protocol/v4/blob/main/programs/squads_multisig_program/src/instructions/config_transaction_create.rs 、 https://github.com/Squads-Protocol/v4/blob/main/programs/squads_multisig_program/src/state/config_transaction.rs 。
