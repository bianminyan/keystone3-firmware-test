#include "gui.h"
#include "gui_views.h"
#include "gui_status_bar.h"
#include "gui_keyboard.h"
#include "gui_button.h"
#include "gui_hintbox.h"
#include "gui_model.h"
#include "user_memory.h"
#include "presetting.h"
#include "gui_wipe_device_widgets.h"
#include "screen_manager.h"
#include "gui_page.h"

static lv_obj_t *g_cont;
static lv_obj_t *g_wipeDeviceHintBox = NULL;
static lv_timer_t *g_countDownTimer;
static int8_t g_countDown = 5;
static PageWidget_t *g_pageWidget;
static bool g_forceWipeDevice = false;
static bool g_wipeDeviceRunning = false;

static void GuiWipeDeviceNVSBarInit();
static void GuiWipeDeviceEntranceWidget(lv_obj_t *parent);
static void WipeDeviceHandler(lv_event_t *e);
static void NotNowHandler(lv_event_t *e);
static void GuiShowWipeDeviceHintBox(void);
static void GuiCreateWipeDeviceHintBox(void);
static void ExecWipeDeviceHandler(lv_event_t *e);
static void WipeDeviceDeal(void);
static void CountDownTimerHandler(lv_timer_t *timer);
static void GuiCountDownDestruct(void *obj, void* param);
static void SetWipeConfirmationButtonState(lv_obj_t *button, bool enabled);

void GuiWipeDeviceSetForced(bool forced)
{
    g_forceWipeDevice = forced;
    printf("wipe-device forced mode set forced=%d\r\n", g_forceWipeDevice);
}

void GuiWipeDeviceWidgetsInit()
{
    g_wipeDeviceRunning = false;
    if (g_forceWipeDevice) {
        printf("wipe-device forced mode init: disable page lock\r\n");
        SetPageLockScreen(false);
    }
    g_pageWidget = CreatePageWidget();
    lv_obj_t *cont = g_pageWidget->contentZone;
    g_cont = cont;
    lv_obj_t *img = GuiCreateImg(cont, &imgWipeDevice);
    lv_obj_align(img, LV_ALIGN_TOP_MID, 0, 36);

    GuiWipeDeviceEntranceWidget(cont);
}

void GuiWipeDeviceWidgetsDeInit()
{
    GuiCountDownDestruct(NULL, NULL);
    GUI_DEL_OBJ(g_wipeDeviceHintBox);
    if (g_cont != NULL) {
        lv_obj_del(g_cont);
        g_cont = NULL;
    }
    if (g_pageWidget != NULL) {
        DestroyPageWidget(g_pageWidget);
        g_pageWidget = NULL;
    }
    if (g_forceWipeDevice && !g_wipeDeviceRunning) {
        printf("wipe-device forced mode deinit before wipe: restore page lock\r\n");
        SetPageLockScreen(true);
        g_forceWipeDevice = false;
    }
}

void GuiWipeDeviceWidgetsRefresh()
{
    GuiWipeDeviceNVSBarInit();
}

void GuiWipeDeviceWidgetsRestart()
{}

static void GuiWipeDeviceNVSBarInit()
{
    if (g_forceWipeDevice) {
        SetNavBarLeftBtn(g_pageWidget->navBarWidget, NVS_LEFT_BUTTON_BUTT, NULL, NULL);
    } else {
        SetNavBarLeftBtn(g_pageWidget->navBarWidget, NVS_BAR_RETURN, CloseCurrentViewHandler, NULL);
    }
}

void GuiWipeDeviceEntranceWidget(lv_obj_t *parent)
{
    lv_obj_t *label = GuiCreateLittleTitleLabel(parent, _("system_settings_wipe_device_wipe_title"));
    lv_obj_align(label, LV_ALIGN_TOP_MID, 0, 140);

    label = GuiCreateNoticeLabel(parent, _("system_settings_wipe_device_wipe_desc"));
    lv_obj_set_width(label, 408);
    lv_obj_set_style_text_align(label, LV_TEXT_ALIGN_CENTER, 0);
    lv_obj_align(label, LV_ALIGN_TOP_MID, 0, 192);

    label = GuiCreateNoticeLabel(parent, _("system_settings_wipe_device_wipe_backup_desc"));
    lv_obj_set_width(label, 408);
    lv_obj_set_style_text_align(label, LV_TEXT_ALIGN_CENTER, 0);
    GuiAlignToPrevObj(label, LV_ALIGN_OUT_BOTTOM_LEFT, 0, 12);

    label = GuiCreateTextLabel(parent, _("system_settings_wipe_device_wipe_button"));
    lv_obj_set_style_text_color(label, lv_color_hex(0xf55831), LV_PART_MAIN);

    lv_obj_align(label, LV_ALIGN_TOP_MID, 0, 541);
    lv_obj_add_flag(label, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(label, WipeDeviceHandler, LV_EVENT_CLICKED, NULL);

}

static void WipeDeviceHandler(lv_event_t *e)
{
    GuiShowWipeDeviceHintBox();
}

static void GuiShowWipeDeviceHintBox(void)
{
    if (g_wipeDeviceHintBox == NULL) {
        GuiCreateWipeDeviceHintBox();
        lv_obj_t *leftBtn = GuiGetHintBoxLeftBtn(g_wipeDeviceHintBox);
        if (g_forceWipeDevice) {
            printf("wipe-device forced mode: hide not-now button\r\n");
            lv_obj_add_flag(leftBtn, LV_OBJ_FLAG_HIDDEN);
            lv_obj_clear_flag(leftBtn, LV_OBJ_FLAG_CLICKABLE);
        } else {
            lv_obj_add_event_cb(leftBtn, NotNowHandler, LV_EVENT_CLICKED, NULL);
        }
        lv_obj_t *rightBtn = GuiGetHintBoxRightBtn(g_wipeDeviceHintBox);
        lv_obj_add_event_cb(rightBtn, ExecWipeDeviceHandler, LV_EVENT_CLICKED, NULL);
        SetWipeConfirmationButtonState(rightBtn, false);
        g_countDown = 5;
        g_countDownTimer = lv_timer_create(CountDownTimerHandler, 1000, rightBtn);
    }
}

static void GuiCreateWipeDeviceHintBox(void)
{
    const uint16_t height = 416;
    const uint16_t sheetTop = 800 - height;

    g_wipeDeviceHintBox = GuiCreateHintBox(height);

    lv_obj_t *img = GuiCreateImg(g_wipeDeviceHintBox, &imgWarn);
    lv_obj_align(img, LV_ALIGN_DEFAULT, 36, sheetTop + 44);

    lv_obj_t *title = GuiCreateLittleTitleLabel(g_wipeDeviceHintBox, _("wipe_device"));
    lv_obj_align(title, LV_ALIGN_DEFAULT, 36, sheetTop + 142);

    lv_obj_t *desc = GuiCreateIllustrateLabel(g_wipeDeviceHintBox,
                                          _("system_settings_wipe_device_wipe_alert_desc"));
    lv_obj_set_width(desc, 408);
    lv_label_set_long_mode(desc, LV_LABEL_LONG_WRAP);
    lv_obj_align(desc, LV_ALIGN_DEFAULT, 36, sheetTop + 194);

    lv_obj_t *leftBtn = GuiCreateTextBtn(g_wipeDeviceHintBox, _("Cancel"));
    lv_obj_set_size(leftBtn, 192, 66);
    lv_obj_align(leftBtn, LV_ALIGN_BOTTOM_LEFT, 36, -24);
    lv_obj_set_style_bg_color(leftBtn, WHITE_COLOR_OPA20, LV_PART_MAIN);

    lv_obj_t *rightBtn = GuiCreateTextBtn(g_wipeDeviceHintBox,
                                          _("system_settings_wipe_device_wipe_start_text"));
    lv_obj_set_size(rightBtn, 192, 66);
    lv_obj_align(rightBtn, LV_ALIGN_BOTTOM_RIGHT, -36, -24);
    lv_obj_set_style_bg_color(rightBtn, DEEP_ORANGE_COLOR, LV_PART_MAIN);
}

static void NotNowHandler(lv_event_t *e)
{
    GuiCountDownDestruct(NULL, NULL);
    GUI_DEL_OBJ(g_wipeDeviceHintBox);
}

static void ExecWipeDeviceHandler(lv_event_t *e)
{
    GuiCountDownDestruct(NULL, NULL);
    GUI_DEL_OBJ(g_wipeDeviceHintBox);
    WipeDeviceDeal();
}

static void WipeDeviceDeal(void)
{
    g_wipeDeviceRunning = true;
    printf("wipe-device start wipe forced=%d\r\n", g_forceWipeDevice);
    if (g_cont != NULL) {
        lv_obj_del(g_cont);
        g_cont = NULL;
    }
    SetPageLockScreen(false);

    g_cont = GuiCreateContainer(lv_obj_get_width(lv_scr_act()), lv_obj_get_height(lv_scr_act()) -
                                GUI_STATUS_BAR_HEIGHT);
    lv_obj_align(g_cont, LV_ALIGN_DEFAULT, 0, GUI_STATUS_BAR_HEIGHT);
    lv_obj_add_flag(g_cont, LV_OBJ_FLAG_CLICKABLE);
    GuiCreateCircleAroundAnimation(g_cont, -84);
    lv_obj_set_size(g_cont, lv_obj_get_width(lv_scr_act()), lv_obj_get_height(lv_scr_act()) -
                    GUI_STATUS_BAR_HEIGHT);

    lv_obj_t *label = GuiCreateTextLabel(g_cont, _("system_settings_wipe_device_generating_title"));
    lv_obj_align(label, LV_ALIGN_TOP_MID, 0, 355);

    label = GuiCreateNoticeLabel(g_cont, _("system_settings_wipe_device_generating_desc1"));
    lv_obj_align(label, LV_ALIGN_TOP_MID, 0, 410);

    label = GuiCreateNoticeLabel(g_cont, _("system_settings_wipe_device_generating_desc2"));
    lv_obj_set_width(label, 408);
    lv_obj_set_style_text_align(label, LV_TEXT_ALIGN_CENTER, 0);
    lv_obj_align(label, LV_ALIGN_TOP_MID, 0, 612);

    GuiModelLockedDeviceDelAllWalletDesc();
}

static void CountDownTimerHandler(lv_timer_t *timer)
{
    lv_obj_t *obj = (lv_obj_t *)timer->user_data;
    char buf[BUFFER_SIZE_16] = {0};
    --g_countDown;
    if (g_countDown > 0) {
        snprintf_s(buf, BUFFER_SIZE_16, _("system_settings_wipe_device_wipe_fmt"), g_countDown);
    } else {
        strcpy_s(buf, BUFFER_SIZE_16, _("system_settings_wipe_device_wipe_end_text"));
    }
    lv_label_set_text(lv_obj_get_child(obj, 0), buf);

    if (g_countDown <= 0) {
        SetWipeConfirmationButtonState(obj, true);
        lv_timer_del(timer);
        g_countDown = 5;
        g_countDownTimer = NULL;
        UNUSED(g_countDownTimer);
    }
}

static void GuiCountDownDestruct(void *obj, void* param)
{
    g_countDown = 5;
    if (g_countDownTimer != NULL) {
        lv_timer_del(g_countDownTimer);
        g_countDownTimer = NULL;
    }
}

static void SetWipeConfirmationButtonState(lv_obj_t *button, bool enabled)
{
    if (enabled) {
        lv_obj_clear_state(button, LV_STATE_DISABLED);
        lv_obj_set_style_bg_color(button, DEEP_ORANGE_COLOR, LV_PART_MAIN);
        lv_obj_set_style_text_opa(lv_obj_get_child(button, 0), LV_OPA_100, LV_PART_MAIN);
        lv_obj_add_flag(button, LV_OBJ_FLAG_CLICKABLE);
    } else {
        lv_obj_add_state(button, LV_STATE_DISABLED);
        lv_obj_set_style_bg_color(button, DEEP_ORANGE_COLOR,
                                  LV_PART_MAIN | LV_STATE_DISABLED);
        lv_obj_set_style_bg_opa(button, LV_OPA_100,
                                LV_PART_MAIN | LV_STATE_DISABLED);
        lv_obj_set_style_text_color(lv_obj_get_child(button, 0), WHITE_COLOR,
                                    LV_PART_MAIN | LV_STATE_DISABLED);
        lv_obj_set_style_text_opa(lv_obj_get_child(button, 0), LV_OPA_100,
                                  LV_PART_MAIN | LV_STATE_DISABLED);
        lv_obj_clear_flag(button, LV_OBJ_FLAG_CLICKABLE);
    }
}
