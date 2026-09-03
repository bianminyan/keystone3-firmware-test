#include "define.h"
#include "rust.h"
#include "keystore.h"
#include "gui_chain.h"
#include "screen_manager.h"
#include "keystore.h"
#include "account_manager.h"
#include "secret_cache.h"
#include "user_memory.h"
#include "gui_chain_components.h"

static bool g_isMulti = false;
static URParseResult *g_urResult = NULL;
static URParseMultiResult *g_urMultiResult = NULL;
static void *g_parseResult = NULL;
static bool g_isSignMessageHash = false;

void GuiSetIotaUrData(URParseResult *urResult, URParseMultiResult *urMultiResult, bool multi)
{
    g_urResult = urResult;
    g_urMultiResult = urMultiResult;
    g_isMulti = multi;
    g_isSignMessageHash = false;
}

#define CHECK_FREE_PARSE_RESULT(result)                                                                                   \
    if (result != NULL)                                                                                                   \
    {                                                                                                                     \
        if (g_isSignMessageHash)                                                                                          \
        {                                                                                                                 \
            free_TransactionParseResult_DisplayIotaSignMessageHash((PtrT_TransactionParseResult_DisplayIotaSignMessageHash)result);                                     \
        } else                                                                                                            \
        {                                                                                                                 \
            free_TransactionParseResult_DisplayIotaIntentData((PtrT_TransactionParseResult_DisplayIotaIntentData)result); \
        }                                                                                                                 \
        result = NULL;                                                                                                    \
    }                                                                                                                     \


void *GuiGetIotaData(void)
{
    CHECK_FREE_PARSE_RESULT(g_parseResult);
    void *data = g_isMulti ? g_urMultiResult->data : g_urResult->data;
    do {
        PtrT_TransactionParseResult_DisplayIotaIntentData parseResult = iota_parse_intent(data);
        CHECK_CHAIN_BREAK(parseResult);
        g_parseResult = (void *)parseResult;
    } while (0);
    return g_parseResult;
}

void *GuiGetIotaSignMessageHashData(void)
{
    CHECK_FREE_PARSE_RESULT(g_parseResult);
    void *data = g_isMulti ? g_urMultiResult->data : g_urResult->data;
    do {
        PtrT_TransactionParseResult_DisplayIotaSignMessageHash parseResult = iota_parse_sign_message_hash(data);
        CHECK_CHAIN_BREAK(parseResult);
        g_isSignMessageHash = true;
        g_parseResult = (void *)parseResult;
    } while (0);
    return g_parseResult;
}

PtrT_TransactionCheckResult GuiGetIotaCheckResult(void)
{
    uint8_t mfp[4];
    void *data = g_isMulti ? g_urMultiResult->data : g_urResult->data;
    GetMasterFingerPrint(mfp);
    PtrT_TransactionCheckResult result = sui_check_request(data, mfp, sizeof(mfp));
    return result;
}

PtrT_TransactionCheckResult GuiGetIotaSignHashCheckResult(void)
{
    uint8_t mfp[4];
    void *data = g_isMulti ? g_urMultiResult->data : g_urResult->data;
    GetMasterFingerPrint(mfp);
    return sui_check_sign_hash_request(data, mfp, sizeof(mfp));
}

void FreeIotaMemory(void)
{
    CHECK_FREE_UR_RESULT(g_urResult, false);
    CHECK_FREE_UR_RESULT(g_urMultiResult, true);
    CHECK_FREE_PARSE_RESULT(g_parseResult);
}

UREncodeResult *GuiGetIotaSignQrCodeData(void)
{
    void *data = g_isMulti ? g_urMultiResult->data : g_urResult->data;
    return SignInternal(iota_sign_intent, data);
}

bool GetIotaIsTransaction(void *indata, void *param)
{
    return !GetIotaIsMessage(indata, param);
}

bool GetIotaIsTransfer(void *indata, void *param)
{
    // Gates the Overview tab. `method` is only ever set for the two shapes
    // extract_transaction_params() can fully decode (Transfer, Stake); an
    // unrecognised transaction leaves it NULL so only Raw Data is shown.
    if (GetIotaIsMessage(indata, param)) {
        return true;
    }
    DisplayIotaIntentData *iota = (DisplayIotaIntentData *)param;
    return iota->method != NULL;
}

bool GetIotaIsMessage(void *indata, void *param)
{
    DisplayIotaIntentData *iota = (DisplayIotaIntentData *)param;
    return !strcmp(iota->transaction_type, "Message");
}

void GuiIotaTxOverviewMessage(lv_obj_t *parent, void *totalData)
{
    DisplayIotaIntentData *txData = (DisplayIotaIntentData *)totalData;
    lv_obj_set_size(parent, 408, 444);
    lv_obj_t *container = CreateSingleInfoTwoLineView(parent, "address", txData->sender);
    lv_obj_align(container, LV_ALIGN_TOP_LEFT, 0, 0);

    container = CreateSingleInfoTwoLineView(parent, "Message", txData->message);
    GuiAlignToPrevObj(container, LV_ALIGN_OUT_BOTTOM_LEFT, 0, 12);
}

void GuiIotaTxOverview(lv_obj_t *parent, void *totalData)
{
    if (GetIotaIsMessage(NULL, totalData)) {
        GuiIotaTxOverviewMessage(parent, totalData);
        return;
    }
    DisplayIotaIntentData *txData = (DisplayIotaIntentData *)totalData;
    lv_obj_set_size(parent, 408, 444);
    lv_obj_add_flag(parent, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(parent, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_clear_flag(parent, LV_OBJ_FLAG_SCROLL_ELASTIC);

    bool isStake = txData->method != NULL && !strcmp(txData->method, "Stake");

    // GuiAlignToPrevObj() is a no-op when its object is parent's only child,
    // leaving it at its default (0,0) creation position -- so every block
    // below can unconditionally chain off "the previous one", first block
    // included, regardless of which of these are actually present.
    lv_obj_t *container = NULL;
    if (txData->gas_sponsored) {
        // Informational, not a warning -- so the neutral grey card, not the
        // red CreateNoticeView that the "max amount" / unknown-transaction
        // warnings use. It also sizes its own height to the wrapped text.
        container = CreateTitledNoticeCard(parent, "Gas Sponsored", _("iota_gas_sponsor_notice"), 408);
        GuiAlignToPrevObj(container, LV_ALIGN_OUT_BOTTOM_LEFT, 0, 12);
    }

    bool maxGasFeeShown = false;
    if (txData->amount != NULL) {
        if (strcmp(txData->amount, "max") == 0) {
            container = CreateNoticeView(parent, 408, 212, _("iota_max_amount_notice"));
        } else {
            container = CreateValueOverviewValue(parent, "amount", txData->amount, "Maximum Gas Fee", txData->max_gas_fee);
            maxGasFeeShown = true;
        }
        GuiAlignToPrevObj(container, LV_ALIGN_OUT_BOTTOM_LEFT, 0, 12);
    }

    if (txData->max_gas_fee != NULL && !maxGasFeeShown) {
        container = CreateSingleInfoView(parent, "Maximum Gas Fee", txData->max_gas_fee);
        GuiAlignToPrevObj(container, LV_ALIGN_OUT_BOTTOM_LEFT, 0, 12);
    }

    if (txData->method != NULL) {
        container = CreateSingleInfoView(parent, "Method", txData->method);
        GuiAlignToPrevObj(container, LV_ALIGN_OUT_BOTTOM_LEFT, 0, 12);
    }

    container = CreateSingleInfoTwoLineView(parent, "sender", txData->sender);
    GuiAlignToPrevObj(container, LV_ALIGN_OUT_BOTTOM_LEFT, 0, 12);

    if (txData->recipient != NULL) {
        container = CreateSingleInfoTwoLineView(parent, isStake ? "Validator" : "To", txData->recipient);
        GuiAlignToPrevObj(container, LV_ALIGN_OUT_BOTTOM_LEFT, 0, 12);
    }
}

void GuiIotaTxRawData(lv_obj_t *parent, void *totalData)
{
    DisplayIotaIntentData *txData = (DisplayIotaIntentData *)totalData;
    lv_obj_clear_flag(parent, LV_OBJ_FLAG_SCROLL_ELASTIC);
    lv_obj_set_size(parent, 408, 444);

    lv_obj_add_flag(parent, LV_OBJ_FLAG_SCROLLABLE);

    // GetIotaIsTransfer() already hides the Overview tab for anything that
    // isn't a decoded Transfer/Stake; here we just flag it so the user isn't
    // left staring at raw JSON with no idea why there's no summary. It's a
    // sibling of the raw-data card, not a child of it.
    if (txData->method == NULL) {
        lv_obj_t *notice = CreateTitledNoticeCard(parent, _("iota_unknown_transaction_title"), _("iota_unknown_transaction_desc"), 408);
        lv_obj_set_style_bg_color(notice, RED_COLOR, LV_PART_MAIN);
        lv_obj_align(notice, LV_ALIGN_TOP_LEFT, 0, 0);
    }

    lv_obj_t *container = CreateContentContainer(parent, 408, 444);
    GuiAlignToPrevObj(container, LV_ALIGN_OUT_BOTTOM_LEFT, 0, 12);
    lv_obj_add_flag(container, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(container, LV_OBJ_FLAG_CLICKABLE);

    lv_obj_t *rawDataLabel = GuiCreateIllustrateLabel(container, txData->details);
    lv_obj_set_width(rawDataLabel, 360);
    lv_obj_align(rawDataLabel, LV_ALIGN_TOP_LEFT, 24, 16);
    lv_obj_set_style_pad_bottom(rawDataLabel, 16, LV_PART_MAIN);
}

UREncodeResult *GuiGetIotaSignHashQrCodeData(void)
{
    void *data = g_isMulti ? g_urMultiResult->data : g_urResult->data;
    return SignInternal(iota_sign_hash, data);
}
