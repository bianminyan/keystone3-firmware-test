# PRAD-8900 修改方案与验证

需求见 [spec.md](spec.md)。

## 提交与修改方案

1. 单独提交需求 spec。
2. 直接修改 production/dev 诊断信息，不 revert 既有提交。
3. 保留原有 LCD 辅助方法、重启倒计时、内核锁定和 Rust panic 行为。

复用已有 `BUILD_PRODUCTION`，不增加构建开关。`src/utils/assert.h` 中 production 的 ASSERT 调用固定空文件名和零行号，不展开 `__FILE__`、`__LINE__`；开发版本保留它们。小写 assert 复用 ASSERT，保留条件求值和失败处理。

`src/utils/assert.c` 在 production 显示通用提示并记录固定断言信息；开发版本额外显示行号和文件路径，文件直接交给 LCD 文本绘制接口换行。保留原有诊断写入顺序和倒计时重启行为。模拟器的 ShowAssert 同步日志规则，不新增 GUI。

## 验证与任务

- [x] 核对断言宏、构建开关和 LCD 输出。
- [x] 恢复此前错误回退的显示辅助及重启行为。
- [x] 实现 production/dev 分支及两种断言入口统一。
- [x] 本地主机测试验证条件只执行一次，失败仍调用处理函数。
- [x] 同一测试文件增加注释行后，production 目标文件逐字节一致；开发版本目标文件保留位置差异。
- [x] Git 差异格式检查。
- [x] 修正方案后的 WEB3 production 固件构建及注释变化 hash 对比。
- [ ] 真机断言显示验证，包括长文件路径排版；开发版本完整构建。
- [ ] 验证 C 断言与 Rust panic 保持原有重启行为。

主机测试使用替身，不代表实际 LCD、Flash 或完整固件 bin 的验证。

## 统一诊断入口

新增 `diagnostic_location.h`，production 使用空文件名和零行号，dev 使用真实位置。ASSERT、CHECK_ERRCODE 系列、内存跟踪、GUI 内存日志、PowerTest、FreeRTOS configASSERT、USB 调试断言和 SQLite 错误定位复用该规则。cryptoauthlib 在 production 省略字符串形式的位置前缀。Rust panic 按现有三种 production feature 使用固定文本，dev 保留位置；不改 panic 后续控制流程。

LVGL 日志在真机配置下关闭；SQLite 的 Unix/Windows 路径不属于 `SQLITE_OS_OTHER` 固件配置。FreeRTOS 协程状态编号和编译期静态断言标识符不替换。此修改不保证消除编译器隐式生成的所有位置元数据。

## Production 固件对比结果

本轮统一诊断入口后的 WEB3 production 构建通过，MPU sandbox ELF 检查通过。

配置为 `BUILD_PRODUCTION=ON`、`BTC_ONLY=OFF`、`CYPHERPUNK=OFF`。使用同一源码路径、构建目录及工具链，基线构建后，仅在 `drv_power.c` 顶部增加一行注释并增量重建，诊断调用行号增加 1；测试结束恢复注释。

- 原始 `mh1903.bin` 前后 SHA-256 均为：`76165cae2e07eef2b2fdf3d3e6cbba36180023bab73e145f04de076fb5a64f6b`。

此次未执行 padding、正式签名或 OTA 发布打包；结论仅覆盖本次注释改动和构建配置，不扩展为任意源码注释修改后固件均一致。
