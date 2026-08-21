#include "mpu_sandbox_task.h"
#include "mpu_sandbox_internal.h"

#include <string.h>

#include "FreeRTOS.h"
#include "mpu_syscall_numbers.h"
#include "task.h"
#ifndef BTC_ONLY
#include "eapdu_framing.h"
#endif

_Static_assert(MPU_SANDBOX_STACK_WORDS * sizeof(StackType_t) == 4096U,
               "sandbox stack must occupy one MPU region");
#ifndef BTC_ONLY
_Static_assert(sizeof(MpuSandboxEapduResult_t) <= MPU_SANDBOX_OUTPUT_SIZE,
               "EAPDU result must fit the sandbox output region");
#endif

#ifndef BTC_ONLY
typedef struct {
    bool has_header;
    uint8_t cla;
    uint16_t command_type;
    uint16_t request_id;
    uint16_t total_packets;
    uint16_t packet_index;
} MpuSandboxTrustedFrame_t;

static void CaptureTrustedEapduFrame(const uint8_t *frame, size_t frame_length);
static bool IsEapduResultValid(const MpuSandboxEapduResult_t *result);
#endif

static StaticTask_t g_mpuSandboxTaskBuffer __attribute__((section("privileged_data")));
static TaskHandle_t g_mpuSandboxTaskHandle __attribute__((section("privileged_data")));
static uint32_t g_mpuSandboxNextRequestId __attribute__((section("privileged_data"))) = 1U;
volatile uint32_t g_mpuSandboxPrivilegedProbe
    __attribute__((section("privileged_data")));
volatile uint8_t g_mpuSandboxSharedHeapProbe
    __attribute__((section(".freertos_heap_probe")));

static StackType_t g_mpuSandboxStack[MPU_SANDBOX_STACK_WORDS]
    __attribute__((section(".mpu_sandbox_stack"), aligned(4096)));
MpuSandboxInput_t g_mpuSandboxInput
    __attribute__((section(".mpu_sandbox_input"), aligned(256)));
MpuSandboxRw_t g_mpuSandboxRw
    __attribute__((section(".mpu_sandbox_rw"), aligned(MPU_SANDBOX_RW_REGION_SIZE)));
#ifndef BTC_ONLY
static uint8_t g_mpuSandboxTrustedPayload[EAPDU_FRAMING_MAX_PAYLOAD_SIZE + 1U];
static MpuSandboxTrustedFrame_t g_mpuSandboxTrustedFrame
    __attribute__((section("privileged_data")));
static bool g_mpuSandboxEapduSuspended __attribute__((section("privileged_data")));
#endif

extern uint32_t __sandbox_text_start;
extern uint32_t _stext;

static const TaskParameters_t g_mpuSandboxTaskParameters = {
    .pvTaskCode = MpuSandboxRuntime,
    .pcName = "mpu_sandbox",
    .usStackDepth = MPU_SANDBOX_STACK_WORDS,
    .pvParameters = NULL,
    .uxPriority = 2U,
    .puxStackBuffer = g_mpuSandboxStack,
    .xRegions = {
        {
            .pvBaseAddress = &g_mpuSandboxInput,
            .ulLengthInBytes = sizeof(g_mpuSandboxInput),
            .ulParameters = portMPU_REGION_READ_ONLY | portMPU_REGION_EXECUTE_NEVER,
        },
        {
            .pvBaseAddress = &g_mpuSandboxRw,
            .ulLengthInBytes = MPU_SANDBOX_RW_REGION_SIZE,
            .ulParameters = portMPU_REGION_READ_WRITE | portMPU_REGION_EXECUTE_NEVER,
        },
        {
            .pvBaseAddress = &__sandbox_text_start,
            .ulLengthInBytes = MPU_SANDBOX_TEXT_REGION_SIZE,
            .ulParameters = portMPU_REGION_READ_ONLY,
        },
    },
    .pxTaskBuffer = &g_mpuSandboxTaskBuffer,
};

BaseType_t xApplicationIsSystemCallAllowed(TaskHandle_t task, BaseType_t task_is_privileged,
                                            uint8_t system_call_number)
    PRIVILEGED_FUNCTION;

BaseType_t xApplicationIsSystemCallAllowed(TaskHandle_t task, BaseType_t task_is_privileged,
                                            uint8_t system_call_number)
{
    if (task == (TaskHandle_t)&g_mpuSandboxTaskBuffer) {
        return system_call_number == SYSTEM_CALL_xTaskGenericNotifyWait ? pdTRUE : pdFALSE;
    }
    return task_is_privileged;
}

bool MpuSandboxTaskCreate(void)
{
    if (g_mpuSandboxTaskHandle != NULL) {
        return true;
    }

    memset(&g_mpuSandboxInput, 0, sizeof(g_mpuSandboxInput));
    memset(&g_mpuSandboxRw, 0, sizeof(g_mpuSandboxRw));
#ifndef BTC_ONLY
    memset(g_mpuSandboxTrustedPayload, 0, sizeof(g_mpuSandboxTrustedPayload));
    memset(&g_mpuSandboxTrustedFrame, 0, sizeof(g_mpuSandboxTrustedFrame));
    g_mpuSandboxEapduSuspended = false;
    EapduFramingReset(&g_mpuSandboxRw.eapdu);
#endif

    return xTaskCreateRestrictedStatic(&g_mpuSandboxTaskParameters, &g_mpuSandboxTaskHandle) == pdPASS;
}

bool MpuSandboxSubmit(MpuSandboxOperation_t operation, const uint8_t *input, size_t input_length,
                      uint32_t *request_id)
{
    uint32_t id;

    if ((g_mpuSandboxTaskHandle == NULL) || (operation == 0) ||
            (input_length > sizeof(g_mpuSandboxInput.input)) ||
            ((input_length > 0U) && (input == NULL))) {
        return false;
    }

    taskENTER_CRITICAL();
    if ((g_mpuSandboxInput.request.operation != 0)
#ifndef BTC_ONLY
            || g_mpuSandboxEapduSuspended
#endif
       ) {
        taskEXIT_CRITICAL();
        return false;
    }

    id = g_mpuSandboxNextRequestId++;
    if (g_mpuSandboxNextRequestId == 0U) {
        g_mpuSandboxNextRequestId = 1U;
    }

    if (input_length > 0U) {
        memcpy(g_mpuSandboxInput.input, input, input_length);
    }
#ifndef BTC_ONLY
    if (operation == MPU_SANDBOX_OP_EAPDU_FRAME) {
        CaptureTrustedEapduFrame(input, input_length);
    }
#endif
    g_mpuSandboxRw.output.response.request_id = id;
    g_mpuSandboxRw.output.response.status = MPU_SANDBOX_STATUS_PENDING;
    g_mpuSandboxRw.output.response.output_length = 0U;
    g_mpuSandboxInput.request.request_id = id;
    g_mpuSandboxInput.request.input_length = (uint32_t) input_length;
    g_mpuSandboxInput.request.fault_address = 0U;
    if (operation == MPU_SANDBOX_OP_FAULT_OTHER_STACK) {
        g_mpuSandboxInput.request.fault_address = (uintptr_t) vTaskStackAddr();
    } else if (operation == MPU_SANDBOX_OP_FAULT_GENERAL_FLASH) {
        g_mpuSandboxInput.request.fault_address = (uintptr_t)&_stext;
#ifndef BTC_ONLY
    } else if (operation == MPU_SANDBOX_OP_EAPDU_FRAME) {
        g_mpuSandboxInput.request.fault_address = (uintptr_t)xTaskGetTickCount();
#endif
    }
    __asm volatile("dmb" ::: "memory");
    g_mpuSandboxInput.request.operation = operation;
    taskEXIT_CRITICAL();

    xTaskNotify(g_mpuSandboxTaskHandle, 1U, eSetBits);
    if (request_id != NULL) {
        *request_id = id;
    }
    return true;
}

bool MpuSandboxGetResponse(uint32_t request_id, MpuSandboxResponse_t *response, uint8_t *output,
                           size_t output_capacity)
{
    uint32_t output_length;

    if ((response == NULL) || (g_mpuSandboxInput.request.operation == MPU_SANDBOX_OP_EAPDU_FRAME) ||
            (g_mpuSandboxRw.output.response.request_id != request_id) ||
            (g_mpuSandboxRw.output.response.status == MPU_SANDBOX_STATUS_PENDING)) {
        return false;
    }

    taskENTER_CRITICAL();
    __asm volatile("dmb" ::: "memory");
    output_length = g_mpuSandboxRw.output.response.output_length;
    if ((output_length > sizeof(g_mpuSandboxRw.output.output)) ||
            (output_length > output_capacity) || ((output_length > 0U) && (output == NULL))) {
        taskEXIT_CRITICAL();
        return false;
    }
    *response = g_mpuSandboxRw.output.response;
    if (output_length > 0U) {
        memcpy(output, g_mpuSandboxRw.output.output, output_length);
    }
    g_mpuSandboxInput.request.operation = 0;
    taskEXIT_CRITICAL();
    return true;
}

#ifndef BTC_ONLY
bool MpuSandboxGetEapduResult(uint32_t request_id, MpuSandboxEapduResult_t *result,
                              const uint8_t **payload)
{
    if ((result == NULL) || (payload == NULL)) {
        return false;
    }

    taskENTER_CRITICAL();
    __asm volatile("dmb" ::: "memory");
    if ((g_mpuSandboxInput.request.operation != MPU_SANDBOX_OP_EAPDU_FRAME) ||
            (g_mpuSandboxRw.output.response.request_id != request_id) ||
            (g_mpuSandboxRw.output.response.status != MPU_SANDBOX_STATUS_OK) ||
            (g_mpuSandboxRw.output.response.output_length != sizeof(*result))) {
        taskEXIT_CRITICAL();
        return false;
    }

    if (!g_mpuSandboxEapduSuspended) {
        vTaskSuspend(g_mpuSandboxTaskHandle);
        g_mpuSandboxEapduSuspended = true;
    }
    taskEXIT_CRITICAL();

    memcpy(result, g_mpuSandboxRw.output.output, sizeof(*result));
    if (!IsEapduResultValid(result)) {
        /* Keep the parser stopped; the caller handles this as a fail-closed fault. */
        return false;
    }
    if (result->payload_length > 0U) {
        memcpy(g_mpuSandboxTrustedPayload, g_mpuSandboxRw.eapdu.packet_data,
               result->payload_length);
    }
    g_mpuSandboxTrustedPayload[result->payload_length] = '\0';
    *payload = g_mpuSandboxTrustedPayload;
    return true;
}

bool MpuSandboxReleaseEapduResult(uint32_t request_id, bool reset_framing)
{
    taskENTER_CRITICAL();
    if ((g_mpuSandboxInput.request.operation != MPU_SANDBOX_OP_EAPDU_FRAME) ||
            (g_mpuSandboxRw.output.response.request_id != request_id) ||
            (g_mpuSandboxRw.output.response.status == MPU_SANDBOX_STATUS_PENDING) ||
            !g_mpuSandboxEapduSuspended) {
        taskEXIT_CRITICAL();
        return false;
    }
    g_mpuSandboxRw.output.response.status = MPU_SANDBOX_STATUS_PENDING;
    taskEXIT_CRITICAL();

    if (reset_framing) {
        EapduFramingReset(&g_mpuSandboxRw.eapdu);
    }
    memset(g_mpuSandboxInput.input, 0, sizeof(g_mpuSandboxInput.input));
    memset(g_mpuSandboxRw.output.output, 0, sizeof(g_mpuSandboxRw.output.output));
    memset(g_mpuSandboxTrustedPayload, 0, sizeof(g_mpuSandboxTrustedPayload));
    memset(&g_mpuSandboxTrustedFrame, 0, sizeof(g_mpuSandboxTrustedFrame));
    taskENTER_CRITICAL();
    g_mpuSandboxRw.output.response.status = MPU_SANDBOX_STATUS_IDLE;
    g_mpuSandboxRw.output.response.output_length = 0U;
    __asm volatile("dmb" ::: "memory");
    g_mpuSandboxInput.request.operation = 0;
    g_mpuSandboxEapduSuspended = false;
    taskEXIT_CRITICAL();
    vTaskResume(g_mpuSandboxTaskHandle);
    return true;
}

bool MpuSandboxResetEapduFraming(void)
{
    taskENTER_CRITICAL();
    if ((g_mpuSandboxInput.request.operation != 0) || g_mpuSandboxEapduSuspended) {
        taskEXIT_CRITICAL();
        return false;
    }
    vTaskSuspend(g_mpuSandboxTaskHandle);
    g_mpuSandboxEapduSuspended = true;
    taskEXIT_CRITICAL();
    EapduFramingReset(&g_mpuSandboxRw.eapdu);
    memset(&g_mpuSandboxTrustedFrame, 0, sizeof(g_mpuSandboxTrustedFrame));
    taskENTER_CRITICAL();
    g_mpuSandboxEapduSuspended = false;
    taskEXIT_CRITICAL();
    vTaskResume(g_mpuSandboxTaskHandle);
    return true;
}

static void CaptureTrustedEapduFrame(const uint8_t *frame, size_t frame_length)
{
    memset(&g_mpuSandboxTrustedFrame, 0, sizeof(g_mpuSandboxTrustedFrame));
    if ((frame == NULL) || (frame_length < EAPDU_FRAMING_HEADER_SIZE)) {
        return;
    }

    g_mpuSandboxTrustedFrame.has_header = true;
    g_mpuSandboxTrustedFrame.cla = frame[0];
    g_mpuSandboxTrustedFrame.command_type = ((uint16_t)frame[1] << 8U) | frame[2];
    g_mpuSandboxTrustedFrame.total_packets = ((uint16_t)frame[3] << 8U) | frame[4];
    g_mpuSandboxTrustedFrame.packet_index = ((uint16_t)frame[5] << 8U) | frame[6];
    g_mpuSandboxTrustedFrame.request_id = ((uint16_t)frame[7] << 8U) | frame[8];
}

static bool IsEapduResultValid(const MpuSandboxEapduResult_t *result)
{
    EapduFramingStatus_t status = (EapduFramingStatus_t)result->framing_status;

    if ((status < EAPDU_FRAMING_WAITING) ||
            (status > EAPDU_FRAMING_ERROR_METADATA_MISMATCH) ||
            (result->timed_out > 1U) ||
            (result->payload_length > EAPDU_FRAMING_MAX_PAYLOAD_SIZE)) {
        return false;
    }

    if (g_mpuSandboxTrustedFrame.has_header) {
        if ((result->cla != g_mpuSandboxTrustedFrame.cla) ||
                (result->command_type != g_mpuSandboxTrustedFrame.command_type) ||
                (result->request_id != g_mpuSandboxTrustedFrame.request_id) ||
                (result->total_packets != g_mpuSandboxTrustedFrame.total_packets) ||
                (result->packet_index != g_mpuSandboxTrustedFrame.packet_index)) {
            return false;
        }
    } else if ((result->cla != 0U) || (result->command_type != 0U) ||
               (result->request_id != 0U) || (result->total_packets != 0U) ||
               (result->packet_index != 0U)) {
        return false;
    }

    if (status == EAPDU_FRAMING_COMPLETE) {
        return (result->total_packets > 0U) &&
               (result->total_packets <= EAPDU_FRAMING_MAX_PACKETS) &&
               (result->packet_index < result->total_packets) &&
               (result->payload_length <=
                ((uint32_t)result->total_packets * EAPDU_FRAMING_MAX_DATA_SIZE));
    }

    if (result->payload_length != 0U) {
        return false;
    }

    if ((status == EAPDU_FRAMING_WAITING) || (status == EAPDU_FRAMING_DUPLICATE)) {
        return (result->total_packets > 0U) &&
               (result->total_packets <= EAPDU_FRAMING_MAX_PACKETS) &&
               (result->packet_index < result->total_packets);
    }

    return true;
}

#endif
