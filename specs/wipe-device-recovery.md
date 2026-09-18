# Wipe Device 后 Setup 状态恢复

## 背景

设备是否已经完成初始化由两类持久化状态共同决定：

- SE 中的账户数据决定当前是否存在可用钱包；
- Flash `DeviceSettings_t.setupStep` 决定启动后进入 Setup 还是正常钱包界面。

删除最后一个钱包或执行设备清理时，这两类状态并不是一次原子写入。账户可能已经从 SE 删除，但 `setupStep` 仍保留为 `SETUP_PAHSE_DONE`。常见窗口包括：

1. 最后一个账户已经删除，异步保存新的 Setup 状态尚未执行；
2. 保存 Device Settings 时擦除或写入 Flash 失败；
3. 删除或清理过程中掉电、复位；
4. SE 账户已不存在，但各账户的 public info Flash 区域仍残留旧数据。
5. 最后账户删除后、`FpWipeManageInfo()` 执行前掉电，旧指纹模板和 `unlockFlag` 仍然保留。

重启后如果只相信 `setupStep == SETUP_PAHSE_DONE`，设备可能在没有有效账户的情况下继续进入正常钱包流程，同时保留旧的公钥、mutable data、multi-sig data 或 RSA 数据。这属于跨存储状态不一致，不能依赖 UI 流程自行修复。

## 目标

- 启动时识别“Setup 已完成，但 SE 中不存在账户”的不一致状态。
- 在进入 UI 和正常钱包操作前清理全部账户槽位的残留 public info。
- 清除旧钱包留下的指纹管理状态，避免新钱包复用相同 PIN 时继承旧指纹权限。
- 只有清理成功后才把 Setup 状态恢复到欢迎页。
- 任一读取、擦除或保存失败都阻止正常启动，避免继续使用不可信状态。
- 正常已初始化设备、未完成 Setup 的设备以及仍有账户的设备不受影响。

## 非目标

- 不重构 `WipeDevice()` 的完整擦除流程或 SE wipe 实现。
- 不改变单个钱包的正常删除流程和账户状态机。
- 不增加新的 UI 页面或错误文案。
- 不把 Device Settings 改造成通用事务存储。本次只补齐当前恢复路径所需的同步错误传播。

## 状态判定

`DeviceSettingsRecoverSetupStep()` 只在下面两个条件同时成立时执行恢复：

```text
setupStep == SETUP_PAHSE_DONE
SE 中存在的账户数量 == 0
```

| 当前状态 | 处理 |
| --- | --- |
| `setupStep != SETUP_PAHSE_DONE` | 不处理，保留原 Setup 进度 |
| `setupStep == SETUP_PAHSE_DONE` 且存在账户 | 不处理，设备状态一致 |
| 查询账户数量失败 | 返回错误，阻止继续启动 |
| `setupStep == SETUP_PAHSE_DONE` 且账户数为 0 | 清理三个账户槽位的 public info，然后恢复到欢迎页 |

这里不能仅根据 public info 是否存在来推断账户有效性。SE 账户状态是账户是否可用的权威来源，Flash public info 只是需要清理的派生数据。

## 解决办法

### 1. 在启动自检阶段恢复

`PowerOnSelfCheck()` 先保留原有的 wipe flag 和 `AccountManagerIsNeedReset()` 恢复流程。不存在待继续的完整设备擦除时，再调用 `DeviceSettingsRecoverSetupStep()`。

该调用发生在 UI 初始化和钱包业务开始之前。函数返回失败时触发 `ASSERT`，不允许设备带着不一致状态进入 Home、签名或账户切换流程。

### 2. 以 `SETUP_PAHSE_DONE` 作为恢复标记

恢复开始时不提前修改 `setupStep`。只要 public info 还没有全部清理，持久化状态仍保持 `SETUP_PAHSE_DONE`，重启后可以再次命中同一恢复条件。

清理顺序如下：

1. 读取 SE 中的账户数量；
2. 对账户槽位 0、1、2 依次调用 `DeleteAccountPublicInfoFlash()`；
3. 调用 `FpWipeManageInfo()` 清除旧指纹模板、签名权限和 `unlockFlag`；
4. 全部成功后把 `setupStep` 改为 `SETUP_PAHSE_WELCOME`；
5. 同步保存 Device Settings；
6. 保存失败时把内存中的状态恢复为 `SETUP_PAHSE_DONE` 并返回错误。

Public info 的扇区擦除是幂等操作，因此擦除中掉电后可以在下次启动重新执行，不会影响恢复正确性。

### 3. 只清理 Flash 派生数据

原有 `DeleteAccountPublicInfo()` 属于正常账户删除路径，除擦除 Flash 外还会更新 RSA hash。启动恢复时已经确认 SE 中没有账户，因此只需要清理可能残留的 Flash 派生数据。

本次将已有的 `DeleteAccountPublicInfoFlash()` 从文件内静态函数改为模块接口，供 `device_settings.c` 调用。它会清理每个账户槽位的：

- account public info；
- mutable data；
- multi-sig data；
- RSA data；
- RAM 中缓存的 public key。

函数保留索引校验和底层擦除错误返回。恢复流程在首个失败处停止，不会误报完成。

### 4. 让 Device Settings 保存结果可检查

原 `SaveDeviceSettingsSync()` 返回 `void`，并忽略 JSON 创建、Flash 擦除和写入结果。恢复流程无法判断 `setupStep` 是否真正落盘。

修改后：

- JSON 创建失败返回 `ERR_GENERAL_FAIL`；
- JSON 根对象、字段或最终字符串创建失败时，在解引用前安全退出；
- Device Settings 扇区擦除失败直接返回错误；
- 长度字段或 JSON 数据未完整写入时返回 `ERR_GD25_BAD_PARAM`；
- 写入完成后分块读回长度字段和完整 JSON（包含结尾 `\0`）并逐字节比较，避免底层写接口忽略 page-program 失败时误报成功；
- `SaveDeviceSettingsAsyncFunc()` 将同步保存结果继续向后台任务返回；
- `DeviceSettingsRecoverSetupStep()` 使用同步保存，确保成功返回前状态已经落盘。

### 5. 下沉 Setup phase 定义

`SETUP_PHASE_ENUM` 原来定义在 UI 的 `gui_setup_widgets.h` 中，但启动恢复逻辑属于 Device Settings 和 boot self-check，不能反向依赖 UI 层。

因此枚举移动到 `device_setting.h`，UI 头文件改为引用该定义。枚举值和原顺序保持不变，避免改变已持久化的 `setupStep` 数值含义。

## 涉及文件

| 文件 | 作用 |
| --- | --- |
| `src/device_settings.c` | 状态判定、残留数据清理、同步保存及错误传播 |
| `src/device_setting.h` | 暴露恢复接口并承载共享的 Setup phase 定义 |
| `src/driver/power_on_self_check.c` | 在 UI 启动前执行恢复，失败时阻止正常启动 |
| `src/crypto/account_public_info.c` | 暴露可返回错误的 Flash-only public info 清理函数 |
| `src/crypto/account_public_info.h` | 声明 `DeleteAccountPublicInfoFlash()` |
| `src/ui/gui_widgets/gui_setup_widgets.h` | 复用 Device Settings 中的 Setup phase 定义 |

## 核心不变量

- `SETUP_PAHSE_DONE` 且账户数为 0 的设备不能进入正常钱包流程。
- `SETUP_PAHSE_WELCOME` 落盘前，三个账户槽位的 public info 必须全部清理成功。
- `SETUP_PAHSE_WELCOME` 落盘前，旧钱包的指纹管理状态必须已经清除。
- 查询账户数量失败不能按“账户数为 0”继续处理。
- public info 擦除或 Device Settings 保存失败不能被忽略。
- 仍存在任一有效账户时，启动恢复不得擦除任何账户数据。
- Setup phase 枚举的持久化数值不能因移动定义而变化。

## Review 重点

- 确认 `PowerOnSelfCheck()` 的调用顺序不会绕过已有 wipe flag 恢复。
- 确认 `GetExistAccountNum()` 返回错误时不会触发清理，也不会继续正常启动。
- 确认三个槽位的地址计算和区域大小不会跨越各自 Flash 边界。
- 确认 RSA 区域在不同固件变体中均应被清理，避免从 WEB3 切换固件后残留数据。
- 确认 `DeleteAccountPublicInfoFlash()` 只做 Flash/RAM 清理，不意外修改仍可能有效的 SE 状态。
- 确认 `SaveDeviceSettingsSync()` 在写入后读回长度和 JSON，能够捕获 `Gd25FlashWriteBuffer()` 未传播的底层 page-program 失败。
- 确认 JSON 内存分配失败不会在 `RemoveFormatChar()` 中触发空指针访问。
- 确认恢复路径调用 `FpWipeManageInfo()` 后，旧指纹不能解锁后续创建的新钱包。
- 确认移动 `SETUP_PHASE_ENUM` 后不存在重复定义、循环依赖或枚举值变化。
- 确认失败路径会停止启动，而不是继续进入 Setup 或 Home 后再暴露半清理状态。

## 回归建议

| 场景 | 操作 | 预期结果 |
| --- | --- | --- |
| 正常已初始化设备 | `setupStep=DONE`，至少存在一个账户，重启 | 不执行恢复，正常进入钱包 |
| 正常 Setup 中 | `setupStep` 为 WELCOME 到 CREATE_WALLET 任一阶段，重启 | 保留原 Setup 进度，不清理账户数据 |
| 最后账户删除后掉电 | 保留 `setupStep=DONE`，SE 账户数为 0，并制造 public info 残留 | 重启清理三个槽位，保存 WELCOME，进入 Setup |
| 指纹清理前掉电 | 删除最后账户后、调用 `FpWipeManageInfo()` 前掉电 | 重启后清除指纹模板及 `unlockFlag`，旧指纹不能解锁新钱包 |
| public info 擦除中掉电 | 在不同槽位和不同区域擦除时断电 | 下次启动重复清理并最终恢复，不进入 Home |
| public info 擦除失败 | 注入扇区擦除失败 | 自检失败，不能继续启动；恢复条件保留 |
| Device Settings 擦除失败 | public info 清理后注入 settings 扇区擦除失败 | 不报告恢复成功，不进入正常 UI |
| Device Settings 短写 | 分别让长度字段和 JSON 写入长度不足 | 返回错误并阻止启动 |
| 账户数量读取失败 | 注入 SE 读取错误 | 不擦除 public info，不把异常当作零账户 |
| 完整 Wipe Device 中断 | 保留 wipe flag 后重启 | 优先执行原完整 wipe 恢复，不进入本恢复分支 |
| 多固件变体 | WEB3、Cypherpunk/非 WEB3 构建各执行一次 | 编译通过，普通和 RSA public info 均无残留 |

破坏性测试应使用可恢复且不承载真实资产的设备，并记录固件提交、SE 代次、注入阶段、错误码及重启后的 Setup/账户状态。

## 当前验证状态

- 分支已 rebase 到 `origin/master` 的 `52a1cf4ce`。
- rebase 后 `git diff --check` 通过。
- MULTI_COINS WEB3 固件构建和 MPU sandbox 布局/调用闭包检查通过。
- 当前基线未提供 `test/wallet_model/run_tests.py`，尚未执行对应主机测试。
- 两代 SE 真机、掉电和 Flash 故障注入回归仍需完成。
