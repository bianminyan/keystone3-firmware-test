# Zcash 大尺寸 UR 扫描支持

关联 Jira：[PRAD-8934](https://keystonejira.atlassian.net/browse/PRAD-8934)

> 本文为已有修改的回溯性需求基线，依据本次讨论与设备日志整理；不表示规格先于实际实现或已经完成验收。

## 背景

Zodl 的 Orchard → Ironwood 迁移会生成较大的签名请求。原 CBOR 校验入口上限为 14 KiB，导致扫描失败。本次实测请求为 `zcash-sign-batch`，完整 CBOR 为 31,378 字节，共 105 个源分片。

## 目标与范围

支持 Cypherpunk 固件处理不超过 32 KiB 的 Zcash 单笔 PCZT 和批量请求，保持输入资源限制和 MPU 隔离。保留 TRON 原有重组总长度及独立资源上限，单帧 UR 文本统一限制为 1536 字节，避免正常重复捕获和源分片计费造成误拒绝。PSRAM 必须不可执行，且不向沙箱开放完整 PSRAM 缓冲区。

不新增迁移业务或签名算法，不开放 512 KiB 请求，不扩大其他类型及 EIP-712 JSON 的容量。

## 需求与场景

### Requirement: 支持合法的大尺寸 Zcash 请求

在满足其余协议与业务规则时，系统必须支持完整 CBOR 不超过 32,768 字节的 `zcash-pczt` 和 `zcash-sign-batch` 请求。

#### Scenario: 扫描并签名迁移请求

- GIVEN Cypherpunk 固件及本次 31,378 字节的合法批量请求
- WHEN 用户完成扫码和交易授权
- THEN 必须能够进入交易确认页面并生成签名结果供钱包接收

#### Scenario: 请求大小边界

- GIVEN 其他条件合法的 Zcash 单笔或批量请求
- WHEN 完整 CBOR 为 32,768 字节
- THEN 不得仅因长度拒绝请求
- WHEN 完整 CBOR 超过 32,768 字节
- THEN 必须在重组前拒绝超限声明

### Requirement: 分片接收正确且资源有界

系统必须校验会话一致性，允许乱序和重复捕获。相同序号不能重复扣除额度。通用上限为 256 个源分片、512 KiB 估算额度；TRON 保留 128 片、192 KiB 额度及原重组总长度限制。包括 TRON 在内的单帧 UR 文本上限为 1536 字节，与重组总长度分别校验。额度不等同于实际内存占用。

#### Scenario: 重复捕获与源分片边界

- GIVEN 14 KiB 通用请求或 32 KiB Cypherpunk Zcash 请求，各由 256 个合法源分片组成
- WHEN 正序或倒序接收全部源分片，期间多次捕获重复帧
- THEN 必须成功重组，不得因重复收费或高估源分片索引数而拒绝

#### Scenario: TRON 重组与沙箱入口边界一致

- GIVEN Web3 固件及满足业务规则的 TRON 多帧请求
- WHEN 完整 CBOR 大于 14 KiB 且不超过 17 KiB（16 KiB sign_data + 1 KiB 元数据）
- THEN 重组成功后必须能够进入沙箱结构校验，不得再被旧 14 KiB 入口限制拒绝
- WHEN 完整 CBOR 超过 17 KiB
- THEN 必须在重组前拒绝超限声明，沙箱入口也必须拒绝超限输入

#### Scenario: 异常输入

- WHEN 分片改变类型、总长度、校验和或片长，或超过允许的数量、长度及资源额度
- THEN 必须拒绝请求
- WHEN CBOR 截断、含多余尾部数据或超过既有结构复杂度限制
- THEN 必须拒绝请求，不进入签名流程

### Requirement: 保持内存隔离

#### Scenario: 正常处理和越权访问

- WHEN 正常固件使用 PSRAM 保存请求并调用沙箱校验
- THEN 合法请求必须能正常处理
- WHEN 尝试执行 PSRAM 数据或由沙箱直接访问未经授权的 PSRAM
- THEN 必须由 MPU 阻止访问
