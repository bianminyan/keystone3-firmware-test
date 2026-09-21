#ifndef _GUI_SCAN_WIDGETS_H
#define _GUI_SCAN_WIDGETS_H

#include "rust.h"

#ifdef CYPHERPUNK_VERSION
void GuiScanSetZcashAddress(URParseResult *result);
#endif

void GuiScanInit(void *param, uint16_t len);
void GuiScanDeInit();
void GuiScanRefresh();
void GuiScanResult(bool result, void *param);
void GuiTransactionCheckPass(void);
#ifdef WEB3_VERSION
void GuiArTransactionReady(const char *address);
#endif
void GuiTransactionCheckFailed(PtrT_TransactionCheckResult result);

#endif /* _GUI_SCAN_WIDGETS_H */
