# PRAD-8895 实现方案与验证

需求基线见 [spec.md](spec.md)。本文补充已有实现的设计选择、验收对应关系和未完成项，不代表新增需求已经获批。

## 修改方案

### 交易页面公共样式

在 `src/ui/gui_chain/gui_chain_components.c/.h` 提供 `GuiSetTransactionContainerStyle`、`GuiSetTransactionFlexStyle` 和 `GuiSetTransactionTitleStyle`，替换各链重复的容器、Flex 和标题样式设置。

公共方法仅设置共有属性，页面保留尺寸、位置及特殊样式覆盖。审阅时需注意 LVGL selector 和设置顺序，不能只比较颜色数值。保留 master 已有的 Monero 动态高度与底部留白，不重新引入字体资源差异。

### 备份确认按钮

`gui_hintbox.c` 复用 `GuiSetConfirmButtonEnabled`，替换本地重复的按钮状态设置。原有复选框判断和继续操作保持不变，对应 spec 的未满足及已满足确认条件两种场景。

### BTC 多签导出提示

在 `gui_hintbox.c/.h` 提供 `GuiCreateSdCardErrorHintBox`，由调用方传入错误类别和关闭回调。多签钱包及签名导出页面复用此方法，保留缺卡与导出失败各自的文案、按钮表现和关闭流程。

## 需单独决定的签名调整

当前实现还将 Solana、Stellar 签名入口改为调用 `SignInternal`。旧入口在 seed 读取失败后执行 assert，公共方法返回失败结果，因此不能将此改动视作纯 UI 去重。

合入前需决定将其拆出，或明确纳入需求后单独审阅。若保留，需验证空返回、签名错误的上层处理，以及敏感缓存清理和锁屏状态恢复。本次补文档不替代此决策，也不修改相关实现。

## 任务与验证

- [x] 独立提交 Jira spec，区分 UI 范围和待确认的签名行为变化。
- [x] 整理交易公共样式、备份按钮状态及 SD 卡提示实现。
- [x] 补充本方案，纳入第二个实现提交。
- [x] 对照原页面逐项确认样式、覆盖顺序、长文本和各链特殊布局。
- [x] 验证备份按钮禁用、启用及反复切换后的操作行为。
- [x] 验证 BTC 多签两类 SD 卡提示及关闭回调。
- [x] 确认签名调整的范围与异常处理政策。
- [x] 完成 WEB3、BTC_ONLY、CYPHERPUNK 固件构建。
- [x] 完成模拟器页面对照和真机相关流程回归。
