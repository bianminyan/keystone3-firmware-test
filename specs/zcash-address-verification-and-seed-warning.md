# Zcash 地址归属验证与助记词安全提醒

## 背景

Zashi 可以轮换收款地址，同一钱包的多个 Unified Address（UA）可能与 Keystone 显示的默认地址不同。仅比较地址字符串不能判断归属，也不能因为 Orchard receiver 属于当前钱包就忽略 UA 中的其他 receiver。

同时，Cypherpunk 固件在用户创建或导入 12 词助记词时，需要明确建议使用 24 词助记词来使用 Zcash，并允许用户理解提醒后继续原流程。

本文描述当前行为契约与验证入口，供实现、QA 和 review 对照。实现与规范冲突时应报告差异，不能用本文代替代码验证。对应分支为 `codex/zcash-optimizations`，功能范围包含 `ffe230336`、`ddad4f65b` 两个提交。

## 目标

- 通过扫描地址二维码验证当前钱包的 Zcash 地址归属，不要求地址等于设备默认收款地址。
- 验证 Orchard receiver 及地址内其他受支持 receiver，避免混合地址被错误标记为当前钱包所有。
- 区分不匹配、地址无效、不支持完整验证及密钥不可用。
- 在 Cypherpunk 的 12 词创建／导入流程弹出 24 词推荐提示，允许返回或继续。
- 保持扫码结果、UFVK 和弹窗状态的生命周期清晰，不引入签名或广播动作。

## 非目标

- 不同步 Zashi 的地址轮换时间，不修改设备默认收款地址。
- 不枚举全部账户或扫描链上余额、交易历史。
- 不增加 Sapling receiver、未知 receiver 或支付 URI 的归属验证。
- 不更改已有助记词、seed、派生算法或钱包资产。
- 不强制用户改为 24 词，不禁止 12 词钱包继续使用 Zcash。
- 不包含 PCZT 交易详情、其他链、fixtures 数据和生成器的修改。

## 请求分类

### 扫码入口

仅在 Rust `cypherpunk` feature 和 C `CYPHERPUNK_VERSION` 对应功能下生效。

`parse_qrcode_text` 将以 `u1`、`utest1`、`uregtest1` 开头的文本识别为地址验证候选，构造 `QRCodeType::ZcashAddressText` 和 `ViewType::ZcashAddressCheck`。前缀匹配不区分 ASCII 大小写，显示时保留扫描原文。这是路由判断，不是地址有效性判断；格式及网络由后续 verifier 校验。

- 保留的二维码文本最多为 1024 字节；超限返回无效消息错误。
- 不剥离 `zcash:` URI，也不提取 URI 参数中的地址。支付请求不能自动变成地址归属验证请求。
- 不新增 multipart UR 地址格式。`decode_ur` / `receive_ur` 对该文本专用类型返回无效消息。
- `handleURResult` 把结果交给扫描页面，`GuiScanResult` 显示验证结果；不进入交易确认或签名页面。

实现入口：`rust/rust_c/src/common/qrcode/mod.rs`、`common/ur.rs`、`src/ui/gui_analyze/gui_resolve_ur.c`、`gui_scan_widgets.c`。

### 网络与账户范围

FFI 使用 `MainNetwork`。测试网或 regtest 地址即使通过扫码前缀路由，也不能在设备上验证为成功。

UI 先读取一次当前设备钱包槽位，仅允许 0、1、2；未登录（255）或其他无效槽位直接显示不可验证，不调用助记词支持检查或 UFVK 读取。通过检查后读取现有 Zcash UFVK，并沿用 `IsZcashSupportedForCurrentMnemonic()` 的支持条件。成功／不匹配文案明确对应当前钱包的 **Zcash Account 0**，不代表搜索了其他 Zcash account。`GetCurrentAccountIndex()` 表示设备钱包槽位，不应与 Zcash 派生账户编号混淆。

此 PR 不修改 UFVK 的生成、存储、解密或 passphrase／SLIP39 支持策略。当前钱包不满足已有支持条件或读取密钥失败时，显示不可验证，不以地址前缀推断归属。

## 修改方式

### 地址归属算法

`address_verification::verify` 依次执行：

1. 对全大写 UA 转为小写供底层库解码，大小写混用的 UA 返回 InvalidAddress；不修改页面显示原文。按指定网络解码地址，要求为 Unified Address。
2. 若存在未知 receiver 或 Sapling receiver，返回不支持完整验证；缺少 Orchard receiver 也返回不支持。
3. 解码当前账户 UFVK，取得 unified incoming viewing key 及 Orchard external incoming viewing key。
4. 使用 Orchard `diversifier_index` 判断 receiver 是否属于该 external viewing key，并恢复 diversifier index。
5. 按扫描地址实际包含的 receiver 类型，在同一个 index 派生预期地址：必须包含 Orchard，省略 Sapling，仅在输入有 transparent receiver 时要求 transparent。
6. 同时比较 Orchard 和 transparent receiver，全部匹配才返回 Owned。

不向后搜索另一个可用 index；不把当前默认地址作为唯一有效地址。Orchard-only 地址可以使用超出 transparent child-index 范围的 diversifier index。

恢复 Orchard index 成功不等于完整地址验证成功：如果 transparent receiver 来自其他钱包或其他 index，必须返回 NotOwned。无法验证的 receiver 不能被忽略后报告成功。

实现入口：`rust/apps/zcash/src/address_verification.rs`。

### 结果分类

| Rust 结果 | 页面含义 |
| --- | --- |
| `Owned` | 地址属于当前钱包的 Zcash Account 0 |
| `NotOwned` | 地址与当前钱包的 Zcash Account 0 不匹配 |
| `InvalidAddress` | 地址编码、结构或网络无效 |
| `UnsupportedReceiver` | 设备不能完整验证该地址格式／receiver 组合 |
| `KeyUnavailable` | 当前钱包不能完成地址验证或所需 viewing key 不可用 |

这些是验证能力和匹配结果，不是链上资金状态。尤其不能把 UnsupportedReceiver 或 KeyUnavailable 显示成“确定属于其他钱包”。

算法按上述顺序返回结果；例如含不支持 receiver 的地址可以在读取 UFVK 前返回 UnsupportedReceiver。不能仅凭某个错误分类推断后续检查已经执行。

### FFI、密钥与扫码内存

- `verify_zcash_address` 将 Rust 结果映射为 C 枚举，空 UFVK 指针返回 KeyUnavailable，空地址指针返回 InvalidAddress。
- `URParseResult` 拥有地址 CString；`zcash_address_qr_text` 只借用该文本，调用方不得单独释放。
- 解码任务将地址复制到 `ZcashAddressScanResult_t` 后释放 Rust QR payload；UI 消息只包含地址字节，不传递 Rust 指针。
- UI 消息队列复制整个结果结构，LVGL 在消息释放前复制显示文本。即使页面关闭、消息被丢弃或发送失败，也不存在跨任务共享 QR 指针或重复释放。
- UFVK 使用局部缓冲区，完成验证后调用 `CLEAR_ARRAY` 清理；不把 UFVK 显示在页面上。

实现入口：`rust/rust_c/src/zcash/mod.rs`、`common/ur.rs::free_ur`、`gui_scan_widgets.c`。

### 12 词助记词提醒

创建和导入流程分别维护弹窗对象及本次流程的接受状态。只有词数为 12、用户尚未接受且不存在同一弹窗时才弹出。

| 入口／操作 | 行为 |
| --- | --- |
| 进入 12 词创建页 | 显示推荐提醒 |
| 创建流程切换词数 | 重置接受状态；切换到 12 词时显示提醒 |
| 进入 12 词导入页 | 显示推荐提醒 |
| 尚未接受就尝试进入下一步 | 再检查提醒状态，不直接推进 |
| 点击 Continue | 标记本次流程已接受，关闭提醒；用户继续正常流程 |
| 创建页点击 Back 或关闭按钮 | 关闭提醒并打开词数选择 |
| 导入页点击 Back 或关闭按钮 | 关闭提醒并退出当前导入视图 |
| 页面销毁后重新进入 | 清理弹窗并重置接受状态，可以再次提示 |
| 非 12 词或非 Cypherpunk 固件 | 不触发本提醒 |

Continue 不是自动生成钱包或立即跳转下一步，也不能替代原有备份确认。本次允许用户继续保留 12 词，不自动将其换成 24 词。

实现入口：`gui_single_phrase_widgets.c`、`gui_import_phrase_widgets.c`、`gui_hintbox.c::GuiCreateZcashSeedWarning`。

## 用户审查页面

### 地址验证结果

显示验证标题、结果说明和扫描到的完整地址。地址自动换行，内容区域支持纵向滚动。仅 Owned 使用绿色，其余状态使用橙色；不能仅依赖颜色表达结果。

地址验证是本地查询操作，不请求交易签名，也不产生可广播交易。验证成功说明支持范围内的 receiver 与当前账户匹配，不表示地址包含资金或已经在链上使用。

### 助记词提醒弹窗

标题：`24 words recommended for Zcash`。

正文：`For better security when using Zcash, we recommend using a 24-word seed phrase.`

通用弹窗计算高度前检查可选说明标签是否存在，单段说明不会解引用空标签。

按钮为 Back / Continue，右上角关闭按钮与 Back 使用相同回调。文案表达推荐，不声称 12 词必然失窃或失效，不增加未在实现中强制执行的词数限制。

### 翻译与字体

新增地址验证和助记词提醒的翻译 key，并同步 `data.csv` 与生成后的 `lv_i18n.c`。当前提供英文、中文文案，其余语言栏使用英文回退，不应描述为全部语言已完成翻译。

更新中文 `cnLittleTitle.c`、`cnText.c`，合并所需字符与 master 已有字符后重新生成，不能覆盖 master 的新字符。仍需在实机检查标题换行、完整地址滚动、按钮和中文字符渲染。

## 构建

`build.sh` 的 CMake 参数由 `-DCYBERPUNK=true` 修正为 `-DCYPHERPUNK=true`，与现有 `CMakeLists.txt` 判断和 `build.py` 一致。

该参数与 Rust 绑定中的 `BUILD_CYBERPUNK` 是不同机制。本独立分支没有带入 transaction-review 的绑定宏统一提交，不能把所有宏重命名列为本 PR 的已完成工作。

## 场景与预期

| 场景 | 预期结果 |
| --- | --- |
| 当前账户默认 UA 或轮换后的 UA | 验证成功，不要求等于默认地址 |
| Orchard-only UA 使用高 diversifier index | 支持，不受 transparent 派生范围限制 |
| 其他钱包的 Orchard receiver 或 internal receiver | 不匹配 |
| 自己的 Orchard + 别人的 transparent | 不匹配，不能部分成功 |
| 自己的 Orchard + 同钱包但不同 index 的 transparent | 不匹配 |
| UA 含 Sapling／未知 receiver，或没有 Orchard | 不支持完整验证，不报告成功 |
| 全大写的有效 UA | 正常验证归属，显示扫描原文 |
| 大小写混用的 UA | 地址无效 |
| 地址 checksum 损坏、错误网络 | 地址无效 |
| `zcash:` URI | 不作为裸地址验证请求处理 |
| 超过 1024 字节的候选文本 | 在保留文本前拒绝 |
| 未登录或设备钱包槽位超出 0–2 | 显示不可验证，不读取 UFVK |
| 当前钱包不支持或 UFVK 不可用 | 显示不可验证 |
| 12 词创建／导入点击 Continue | 允许继续，仍执行原有流程要求 |
| 创建页 Back／关闭 | 返回词数选择，不自动变成 24 词 |
| 导入页 Back／关闭 | 退出当前导入视图 |
| 24 词、其他非 12 词或非 Cypherpunk | 不显示新增提醒 |

## 核心不变量

- Owned 必须表示该地址全部受支持 receiver 匹配同一个 external account 和 index。
- 不支持的 receiver 不能被忽略，Orchard 匹配不能替代整条 UA 验证。
- 地址轮换不影响同一账户的归属判断，不依赖准确时钟或地址字符串相等。
- 不匹配、格式无效、不支持和密钥不可用必须分开表达。
- 提醒不修改助记词，不替代备份确认，也不强制禁止 12 词。
- QR payload、UFVK 缓冲区及弹窗状态在其生命周期结束时清理。

## 涉及文件

| 文件 | 职责 |
| --- | --- |
| `rust/apps/zcash/src/address_verification.rs`、`lib.rs` | 地址归属算法与单测 |
| `rust/rust_c/src/common/qrcode/mod.rs`、`common/ur.rs` | 裸地址分类、视图类型、payload 释放及 FFI 回归测试 |
| `rust/rust_c/src/zcash/mod.rs` | 验证 API、返回枚举和文本借用接口 |
| `src/ui/gui_analyze/gui_resolve_ur.c` | 扫码结果分发 |
| `src/ui/gui_widgets/gui_scan_widgets.c`、`gui_scan_widgets.h` | 当前钱包 UFVK、结果页及扫码对象生命周期 |
| `src/ui/gui_widgets/gui_single_phrase_widgets.c` | 创建流程提醒和词数切换 |
| `src/ui/gui_widgets/gui_import_phrase_widgets.c` | 导入流程提醒 |
| `src/ui/gui_components/gui_hintbox.c`、`gui_hintbox.h` | 通用 Zcash 提醒弹窗 |
| `src/ui/lv_i18n/data.csv`、`lv_i18n.c` | 提醒和验证结果文案 |
| `src/ui/gui_assets/font/cn/cnLittleTitle.c`、`cnText.c` | 中文字形 |
| `build.sh` | Cypherpunk CMake 参数修正 |

## Review 重点

- 核对成功结果是否覆盖整个 UA，尤其是混合 receiver、不同 index 和 internal 地址。
- 核对主网限制、Account 0 文案、设备钱包槽位与 Zcash 账户编号的区别。
- 核对扫码路由不接受支付 URI、不进入签名，并在退出／重扫时释放 payload。
- 核对 Back、关闭、Continue、词数切换和重新进入时的弹窗状态；接受提醒不能跳过原有备份步骤。
- 不把 24 词产品建议写成已证明 12 词不安全的结论；此 PR 不包含密码学安全性研究。
- 本分支没有 PCZT 展示优化、fixtures 子仓库或绑定宏统一，不应在 PR 描述中混入这些范围。

## 验证

从仓库根目录运行：

```sh
cargo test --manifest-path rust/Cargo.toml -p app_zcash address_verification --lib
cargo test --manifest-path rust/Cargo.toml -p rust_c --no-default-features --features simulator-cypherpunk --lib zcash_address
python3 tools/tests/test_zcash_address_scan.py
git diff --check
```

核心测试入口：

- `official_mainnet_vector_is_owned`：Zcash 官方固定 UFVK／UA 向量（commit `78321beacb0e0477e33cd002b56585a107c2708c`），覆盖小写、大写及错误网络。
- `default_and_rotated_addresses_are_owned`：默认地址及轮换 index。
- `orchard_only_supports_full_diversifier_range`：Orchard-only 高 index。
- `other_wallet_and_internal_address_are_not_owned`：其他钱包与 internal 地址。
- `mixed_receivers_and_wrong_transparent_index_are_rejected`：混合 receiver、不同 transparent index。
- `unverified_receivers_never_report_success`：Sapling、未知 receiver 和不能完整验证的地址。
- `uppercase_addresses_are_owned_but_mixed_case_is_invalid`：全大写地址、混合大小写、损坏编码和大写测试网地址。
- `invalid_encoding_network_and_viewing_key_are_rejected`：坏编码、错误网络及无效 UFVK。
- `zcash_address_text_qr_owns_and_releases_its_payload`：扫码文本到 FFI 验证、重复释放流程、URI／超长输入及空指针分类。

- `tools/tests/test_zcash_address_scan.py`：编译实际扫码函数的账户检查部分，遍历所有 uint8_t 槽位及助记词／密钥可用状态，验证无效槽位不访问密钥；另覆盖地址消息在 Rust 结果释放后仍有效、发送失败路径，以及可选弹窗标签的所有组合。后两项启用 AddressSanitizer 和 UndefinedBehaviorSanitizer。

Review 修复后通过 8 项算法单测、1 项扫码 FFI 单测、3 项 C 回归测试，以及相关 C 文件和生成字体的语法检查。没有为弹窗交互新增自动化测试，语法检查不等于完整固件构建或视觉验收。

尚未做实机验证。QA 应分别测试主网默认／轮换地址、错误钱包、混合地址、错误网络和连续扫码；在创建与导入流程核对 12 词提醒、Continue、Back／关闭、切换到 24 词及重新进入，并回归非 Cypherpunk 固件。
