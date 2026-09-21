# TRON 交易审查与签名重构

## 背景

TRON 交易请求来自不可信主机或二维码。`TriggerSmartContract` 除 calldata 外，还可以携带原生 TRX 的 `call_value`，以及 TRC-10 的 `call_token_value`、`token_id`。如果确认页只显示 calldata 中的 TRC-20 金额，或把普通合约调用显示为 `0 TRX`，用户看到的资产支出就与实际签名内容不一致。

本次改动同时收紧交易结构及签名账户校验，补齐附带资产、交易头、Memo 和调用数据的审查能力。**最终行为是展示合法附带资产后允许签名，不因附带金额非零或 selector 未识别而直接拒绝。** 已识别指令的损坏数据与未知调用使用不同处理路径。

本文描述当前行为契约与验证入口，供实现、QA 和 review 对照。实现与规范冲突时应报告差异，不能用本文代替代码验证。对应分支为 `codex/tron-optimizations`，功能范围包含 `b233e022d`、`f2c6c13c3` 两个提交。

## 目标

- 解析、账户检查和最终交易签名采用一致的结构校验。
- 签名前验证请求指纹、派生路径与实际 seed 对应的发送者地址。
- 同时展示 TRC-20 业务金额与合约调用附带资产，明确两者各自的接收地址。
- 对未知合约调用保留风险提示和完整原始 calldata，不伪造可信业务解释。
- 使用整数／字符串格式化金额，保留二进制 Memo 和原始交易哈希。
- 提供动态布局和长内容分页，避免固定高度及固定缓冲区造成内容遗漏。

## 非目标

- 不增加链上查询、合约执行模拟、ABI 下载或交易广播。
- 不支持所有 TRON 合约类型，也不放开一笔 raw transaction 中的多个 contract。
- 不推测附带 TRC-10 的代币名称、精度或法币价值。
- 不重构 PersonalMessage 的分类和签名协议；交易专用规则不能直接等同于消息签名规则。
- 不包含其他链、fixtures 数据、生成器或 UI baseline 工具的迁移。

## 请求分类

### 原始交易请求

`tron-sign-request` 的交易入口通过 `decode_to_wrapped` 解码 `RawData`，要求重新编码后的字节与请求 `sign_data` 完全一致，再进入 `WrappedTron::from_raw_transaction`。

`parse_tx_request`、`check_tx_request`、`sign_tx_request` 都经过该入口。最终签名不能仅依赖扫描阶段曾经检查成功。

| 合约类别 | 行为 |
| --- | --- |
| `TransferContract` | 校验地址及非负金额，展示原生 TRX 转账 |
| `TransferAssetContract` | 校验地址、资产标识及非负金额，保留 TRC-10 转账支持 |
| `TriggerSmartContract`，规范 `transfer` / `approve` | 展示 TRC-20 金额、目标地址及附带资产 |
| `TriggerSmartContract`，其他 selector、空或不足 4 字节的数据 | 进入普通 Contract Call 审查，展示警告、合约、附带资产及原始调用数据 |
| 其他合约类型 | 返回不支持错误 |

实现入口：`rust/apps/tron/src/lib.rs`、`transaction/wrapped_tron.rs`。

### Legacy Keystone 请求

保留已有 `Payload` 构造交易的入口。`sign_raw_tx` 在签名前执行 `tx.check(&context)`，并在实际签名时再次核对 seed 对应地址。

此入口根据结构化 payload 构造 protobuf 交易，不能将其描述为“直接签名 UR 原始 sign_data”。原始交易入口的字节一致性保证和 legacy 构造流程应分别核对。

## 修改方式

### 路径、钱包归属与签名人

交易和 PersonalMessage 的路径形态均为 `m/44'/195'/0'/0/index`，根标记也接受 `M`。account 固定为 `0'`，change 固定为 `0`，index 必须能解析为小于 `2^31` 的非 hardened `u32`；缺失、额外层级及错误 coin type 拒绝。

UR `CryptoKeyPath` 不携带文本根前缀，FFI 的 `request_path` 补上 `m/` 并执行公共路径校验，覆盖检查、解析和签名入口。缺失路径不再回退默认路径。PersonalMessage 的 From 与交易地址共用公钥派生逻辑；派生失败直接返回错误，不再隐藏 From 后继续。直接调用消息签名函数也必须通过路径校验。

- 扫描检查要求 master fingerprint 长度为 4，并与请求 source fingerprint 匹配。
- 最终 UR 签名从实际 seed 计算 fingerprint，缺失或不匹配拒绝；该检查同时覆盖交易和 PersonalMessage 分支。
- `tron_sign_request` 在签名闭包结束后、构造返回结果前统一清零调用方传入的 seed 缓冲区，覆盖成功和错误返回；C 调用方原有的清理仍保留。
- 交易签名从 seed 和路径派生公钥，计算 TRON 地址并核对交易 owner。fingerprint 不能替代发送者地址校验。
- Legacy payload 还核对请求 xfp 与解析上下文，最终交易签名同样核对 seed 对应地址。

实现入口：`address.rs::check_hd_path`、`signer.rs::validate_seed_signer`、`rust_c/src/tron/mod.rs`。

### 交易结构与金额合法性

- raw transaction 必须包含且只包含一个 contract。
- `ref_block_bytes` 长度为 2，`ref_block_hash` 长度为 8。
- timestamp 必须大于 0，expiration 必须大于 timestamp，fee limit 不得为负。
- owner、收款人及合约地址按对应合约类型校验：21 字节且前缀为 `0x41`。
- 合约类型必须与 `Any.type_url` 一致，参数解码后重新编码必须与输入一致。
- 非空 provider、非空 contract_name、负数 permission ID 拒绝。
- 普通转账及合约附带资产的金额不得为负。

这些是本地结构及一致性检查，不验证引用区块是否存在，也不表示设备已确认交易相对于当前链上时间仍未过期。

### 合约调用与附带资产

所有 `TriggerSmartContract` 都先解析附带资产，再区分 calldata 类型。

| 字段 | 校验与展示 |
| --- | --- |
| `call_value` | 非负；按 6 位精度精确格式化为 TRX，作为独立附带资产字段；零值也显示 |
| `call_token_value` | 非负；存在非零 token ID 时展示原始整数数量，不假定代币精度 |
| `token_id` | 非负；非零时展示；附带 TRC-10 数量大于 0 但 ID 为 0 时拒绝 |
| `data` | 保留全部字节，以 `0x` 前缀十六进制展示；空数据展示为 `0x` |

非零 token ID 配合零 token 数量允许进入审查，显示 ID 和数量 `0`。不能把“数量非零但没有 ID”当成普通无资产调用。

已识别的 selector：

- `a9059cbb`：TRC-20 Transfer。
- `095ea7b3`：TRC-20 Approve。

两种已识别调用都要求 calldata 恰好为 68 字节，地址参数的高 12 字节为零。截断、尾随数据和非法 padding 拒绝，不能回退为普通 Contract Call。

其他 selector，以及不足 4 字节或空 calldata，使用 Contract Call 布局：主金额为实际 `call_value`，To 为合约地址，并携带无法确定调用效果的警告。展示原始数据只提供审查能力，不代表设备已验证调用安全。

TRC-20 的 Value 仍来自 calldata，To 为 token recipient／approve spender；附带 TRX 和 TRC-10 的接收者是合约地址，必须另外标明。不得把附带 TRX 覆盖到 TRC-20 金额上，也不得只显示其中一种资产。

实现入口：`wrapped_tron.rs::from_raw_transaction` 的 `TriggerSmartContract` 分支。

### 金额格式化与 Memo

`format_decimal_units` 使用十进制字符串插入小数点、移除无意义末尾零，避免 `f64` 舍入和科学计数法。U256 的 TRC-20 数量不能先降为机器整数或浮点数。

已知 TRC-20 按内置合约地址表获取名称和精度；未知 TRC-20 显示 `TRC20 Token (raw)` 和原始数量。内置元数据不是合约代码真实性或安全性的证明。附带 TRC-10 始终按原始数量与 ID 展示。

Memo 同时保留：

- 合法 UTF-8 且不包含 NUL 时的文本。
- 全部原始字节的十六进制。
- 原始字节的 SHA-256。

非 UTF-8 或包含 NUL 时不提供文本，但仍保留 hex 和哈希。空 Memo 隐藏相关卡片；二进制 Memo 不得因文本为空而被全部隐藏。

### FFI 与内存生命周期

解析结果通过 `DetailTx` → `DisplayTronDetail` 传入 C，新增附带资产字段为 `call_value`、`call_token_value`、`token_id`、`call_data`、`contract_warning`。交易头和 Memo 字段同样贯穿 Rust、FFI、getter 和模板。

每个新增字符串都在 `DisplayTronDetail::free` 中释放。长 calldata 的 getter 使用 `GetTrxCallDataLen` 分配包含结尾 NUL 的完整缓冲区，不能沿用固定长度标签缓冲区。

分页控件借用文本源；其容器删除事件负责释放文本，不能在仍可翻页时提前释放。

## 用户审查页面

### Overview

普通 TRON 与 TRON Swap 模板均提供附带资产卡片：

1. 未知调用警告，位于其他卡片之前；已识别调用隐藏该卡片。
2. Attached TRX，以及 Attached assets recipient（合约地址）。
3. 存在 token ID 时，显示 Attached TRC-10 amount (raw units) 和 TRC-10 Token ID。
4. 保留原有 Value／Swap、Method、From、To 等内容。

附带资产卡片仅在合约调用时出现。普通 TRX 转账不能因新增展示逻辑而出现重复的“附带资产”。

例如，TRC-20 转账携带 `call_value = 1_000_000_000_000` sun 时，原 token 数量保持不变，并单独展示 `1000000 TRX`；普通 Contract Call 的主 Value 也必须为 `1000000 TRX`，不能仍为 `0 TRX`。两处出现的该 TRX 数值表示同一笔附带金额，不是两笔支出。

### Details

普通 TRON 详情保留金额、方法、地址、合约、token、原始数量和网络，并补充：

- Fee Limit（sun）、Timestamp / Expiration（ms）。
- Permission ID、Ref Block Bytes / Hash。
- Memo 文本、Memo Hex、Memo SHA-256。
- Raw Data SHA-256。

普通 TRON 和 Swap 的 Details 均包含附带资产、接收合约、未知调用警告，以及可分页的 Call Data（hex）。Swap 保留其原有专用详情；不能声称其已经具有普通 TRON 的全部交易头卡片。

Raw Data SHA-256 对应交易 raw_data 的哈希，不是 calldata 或整个签名封装的哈希。它不能替代金额、地址和调用数据展示。

### 动态布局与当前边界

模板通过 `dynamic` 显式启用纵向内容布局，并使用 `paged_text` 展示长 Memo 和 calldata。此公共能力由 TRON 模板使用，本次未替换其他链的页面模板。

固件和模拟器共用 `gui_trx_template.h` 中的交易、Swap 和 PersonalMessage 模板，TRON 页面不再依赖本地未跟踪的 `page_trx*.json`。模拟器仍支持其他链原有的外部模板文件。

新增标签及未知调用警告当前为英文，本次没有完成新的多语言词条和字体生成。已经通过的结构及 getter 测试不等于在每个语言、字体和实机分辨率下完成视觉验收。

## 场景与预期

| 场景 | 预期结果 |
| --- | --- |
| TRC-20 transfer / approve 附带非零 TRX | 保留 token 金额，另列附带 TRX 和接收合约；允许确认签名 |
| 合约调用附带 TRC-10 | 展示原始数量与 token ID；允许确认签名 |
| 数量为零、token ID 非零 | 展示零数量与 ID，不直接拒绝 |
| 普通 Contract Call 附带 1,000,000 TRX | 主金额和附带金额均准确，显示警告与原始 calldata |
| 空、1～3 字节或未知 selector calldata | 普通 Contract Call 审查，无越界访问 |
| 已识别 selector 的 calldata 截断、超长或 padding 非零 | 拒绝，不降级 |
| 负数附带金额／token ID，或非零 TRC-10 数量缺少 ID | 拒绝 |
| 多 contract、错误 type_url、非规范 protobuf | 解析、检查和最终交易签名路径拒绝 |
| 请求 fingerprint 缺失或与实际 seed 不一致 | 最终 UR 签名拒绝，清零传入的 seed |
| PersonalMessage 的 coin type、account、change 或路径层级错误 | 检查、解析及最终签名拒绝 |
| PersonalMessage 无法从当前 xpub 派生公钥 | 检查和解析返回错误，不隐藏 From 后继续 |
| 交易 owner 与实际 seed 派生地址不一致 | 最终交易签名拒绝 |
| 非 UTF-8 或包含 NUL 的 Memo | 文本隐藏，hex 和哈希保留 |
| 超过固定标签缓冲区的 calldata | 完整分配、复制并分页展示，末尾字节不丢失 |

## 核心不变量

- 非零附带资产必须在确认页披露，不能以 TRC-20 主金额或 `0 TRX` 掩盖。
- token recipient／spender 与附带资产接收合约必须区分。
- 已支持指令解码失败与未知调用是不同状态，前者不能降级放行。
- 对原始交易入口，编码一致性校验保证签名哈希对应收到的 raw sign_data；展示字符串不参与交易重建。
- fingerprint 校验不能替代实际 seed 对应的 owner 校验。
- 金额精度和 Memo 原始字节不得因 UI 格式化而丢失。
- FFI 新增字符串必须成对分配、释放；分页源存活到控件销毁。

## 涉及文件

| 文件 | 职责 |
| --- | --- |
| `rust/apps/tron/src/address.rs` | 路径策略、地址派生与格式 |
| `rust/apps/tron/src/lib.rs` | raw 入口的编码一致性、解析／检查／签名入口 |
| `rust/apps/tron/src/transaction/wrapped_tron.rs` | 合约解析、附带资产、金额格式化、交易头及回归测试 |
| `rust/apps/tron/src/transaction/parser.rs` | Overview / Detail 展示数据 |
| `rust/apps/tron/src/transaction/signer.rs` | seed 指纹、owner 校验及交易签名 |
| `rust/rust_c/src/tron/mod.rs`、`structs.rs` | UR 路径／指纹处理、FFI 数据转换和释放 |
| `src/ui/gui_chain/multi/web3/gui_trx.c`、`gui_trx.h` | 字段 getter、显示条件、长数据长度 |
| `src/ui/gui_analyze/multi/web3/gui_general_analyze.c`、`gui_general_analyze.h`、`gui_trx_template.h` | 回调映射及固件／模拟器共用的 TRON 模板 |
| `src/ui/gui_analyze/gui_analyze.c` | 显式启用的动态布局、分页控件及源内存生命周期 |
| `tools/tests/test_tron_attached_assets.py` | 页面字段和 C getter 回归检查 |

## Review 重点

- 从最终代码判断接受策略，不能沿用“非零 call_value 一律拒绝”的旧描述。
- 核对两个 tab 均展示附带资产，并标明合约接收者；不能只增加 Rust 字段而漏掉 FFI 或模板。
- 核对未知 selector 的警告和原始数据，以及已识别 selector 的严格长度检查。
- 核对直接调用最终签名入口时仍校验编码、路径、fingerprint 和 owner。
- 检查 i64 / U256 边界、1 sun、小数格式、长 hex、含 NUL 的 Memo 和隐藏卡片后的布局。
- 当前只验证本地字段，不证明合约执行效果、余额充足、token 精度或链上有效性。

## 验证

从仓库根目录运行：

```sh
cargo test --manifest-path rust/Cargo.toml -p app_tron --lib
cargo test --manifest-path rust/Cargo.toml -p rust_c --no-default-features --features simulator-multi-coins --lib tron::tests
cargo check --manifest-path rust/Cargo.toml -p rust_c --no-default-features --features simulator-multi-coins
python3 -m unittest discover -s tools/tests -p test_tron_attached_assets.py
git diff --check
```

核心测试入口：

- `displays_attached_assets_through_parse_check_and_sign`：普通调用、transfer、approve 保留附带金额；通过公开解析／检查／签名入口，并恢复签名公钥，核对签名确实对应审查的 raw bytes。
- `displays_empty_short_and_long_contract_data_without_truncation`：空、短以及 4096 字节调用数据，含 1 sun 的精确展示。
- `rejects_negative_or_unidentified_attached_assets`：负数字段和缺少 ID 的附带 TRC-10。
- `rejects_non_canonical_trc20_calldata`：已识别指令的尾随数据及非法 padding。
- `formats_u256_amount_without_float_rounding`、`preserves_non_text_memo_as_hex_and_hash`：金额精度及二进制 Memo。
- `personal_message_rejects_invalid_derivation_paths`、`personal_message_display_matches_recovered_signer`：拒绝错误消息签名路径，并核对正常路径的展示地址与恢复出的签名公钥一致。
- `rust_c::tron::tests`：交易／消息请求的路径校验、UR 根前缀恢复和公钥派生错误；直接调用最终 FFI 签名入口验证缺失／错误 fingerprint 被拒绝，正确 fingerprint 保持原签名结果，并检查成功、路径错误、seed 错误、交易解析和 owner 错误后的 seed 清零。
- `test_tron_attached_assets.py`：两个模板的 Overview / Details 资产字段、警告顺序、calldata 分页配置、实际 C getter 对长数据的完整复制，以及模拟器无需外部文件即可加载共用模板。

当前已通过 73 项 TRON Rust 单测、6 项 TRON FFI 单测、3 项 UI 回归检查、FFI 编译，以及相关 C 文件的语法检查。C 检查使用生成后的 multi-coins Rust 绑定头和模拟器编译参数，不等于完整固件链接或实机运行。

尚未完成实机视觉及交互验收。QA 应扫描普通合约、transfer、approve 的附带资产请求，核对概览、详情、末页 calldata 和最终确认流程；也应回归普通 TRX / TRC-10 转账、Swap 与 PersonalMessage，确认新增卡片的隐藏和显示条件。
