# PRAD-8896 修复方案与任务

## 原因

`DecodeMnemonics` 成功返回 `shard->valueLength`，`Slip39CheckFirstWordList` 原样返回该长度。20/33 词对应 16/32 字节，并非 `SUCCESS_CODE`。

`ModelSlip39WriteEntropy` 将其与零比较，导致有效分片在恢复校验和账户写入前退出。`GetSlip39MnemonicsWords` 和 `Slip39GetMasterSecret` 成功返回零，不应一起改成长度判断。

## 修改方案

`Slip39CheckFirstWordList` 检查词数及解码长度，失败返回负错误码，成功输出阈值并返回 `SLIP39_OK`。头文件记录返回约定。`DecodeMnemonics` 仍返回长度。

`ModelSlip39WriteEntropy` 使用普通成功码判断，不再用 `shareLen` 或额外归一化赋值。另一个调用方 `gui_mnemonic_input.c` 按 `ret < 0` 判断失败，不消费长度，仍兼容；其共用分支调用的 `Slip39OneSliceCheck` 未改动，不能将共用判断直接改为非零失败。

## 任务与验证

- [x] 核对返回值契约及调用路径。
- [x] 先单独提交 spec 固化需求。
- [x] 在分片校验接口内部验证长度并统一成功返回零。
- [x] 本地提取实际校验接口和创建函数，以解码替身验证接口与创建控制流。
- [x] 用例覆盖两种长度的成功路径、负错误码、零返回、长度不匹配、无效阈值、缺片、恢复错误及 entropy/EMS 不一致；同时检查是否写入账户、是否异常退出和锁屏状态恢复。

本地控制流测试为辅助证据，脚本未纳入仓库，不作为共享可复现测试命令；不代表真机或密码学集成测试已通过。
