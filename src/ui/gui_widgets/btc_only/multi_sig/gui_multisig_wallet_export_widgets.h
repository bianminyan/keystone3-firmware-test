#ifndef _GUI_MULTI_SIG_IMPORT_WALLET_SUCCESS_WIDGETS_H
#define _GUI_MULTI_SIG_IMPORT_WALLET_SUCCESS_WIDGETS_H

void GuiMultisigWalletExportWidgetsInit(char* verifyCode, uint16_t len);
void GuiMultisigWalletExportWidgetsDeInit();
void GuiMultisigWalletExportWidgetsRefresh();
void GuiSetExportMultiSigSwitch();
bool GuiGetExportMultisigWalletSwitch();
// true: `address` holds a NUL-terminated address. false: `address` is "" and the caller must fail closed.
bool ModelGenerateMultiSigAddress(char *address, uint32_t maxLen, char *walletConfig, uint32_t index);

#endif
