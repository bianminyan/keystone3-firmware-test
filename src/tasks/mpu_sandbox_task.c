#include "mpu_sandbox_task.h"

#include <string.h>

#include "FreeRTOS.h"
#include "sha256.h"
#include "task.h"

#define MPU_SANDBOX_STACK_WORDS    1024U
#define MPU_SANDBOX_INPUT_SIZE     240U
#define MPU_SANDBOX_OUTPUT_SIZE    244U
#define MPU_SANDBOX_ARENA_SIZE     256U

typedef struct {
    volatile MpuSandboxRequest_t request;
    uint8_t input[MPU_SANDBOX_INPUT_SIZE];
} MpuSandboxInput_t;

typedef struct {
    volatile MpuSandboxResponse_t response;
    uint8_t output[MPU_SANDBOX_OUTPUT_SIZE];
} MpuSandboxOutput_t;

_Static_assert(MPU_SANDBOX_STACK_WORDS * sizeof(StackType_t) == 4096U,
               "sandbox stack must occupy one MPU region");
_Static_assert(sizeof(MpuSandboxInput_t) == 256U,
               "sandbox input must occupy one MPU region");
_Static_assert(sizeof(MpuSandboxOutput_t) == 256U,
               "sandbox output must occupy one MPU region");
_Static_assert(MPU_SANDBOX_ARENA_SIZE == 256U,
               "sandbox arena must occupy one MPU region");

static void MpuSandboxTask(void *argument);
static void MpuSandboxProcessRequest(MpuSandboxInput_t *input, MpuSandboxOutput_t *output,
                                     uint8_t *arena);
static void MpuSandboxTriggerFault(MpuSandboxOperation_t operation, MpuSandboxInput_t *input,
                                   MpuSandboxOutput_t *output, uint8_t *arena);

static StaticTask_t g_mpuSandboxTaskBuffer __attribute__((section("privileged_data")));
static TaskHandle_t g_mpuSandboxTaskHandle __attribute__((section("privileged_data")));
static uint32_t g_mpuSandboxNextRequestId __attribute__((section("privileged_data"))) = 1U;
static volatile uint32_t g_mpuSandboxPrivilegedProbe
    __attribute__((section("privileged_data")));
static volatile uint8_t g_mpuSandboxSharedHeapProbe
    __attribute__((section(".freertos_heap_probe")));

static StackType_t g_mpuSandboxStack[MPU_SANDBOX_STACK_WORDS]
    __attribute__((section(".mpu_sandbox_stack"), aligned(4096)));
static MpuSandboxInput_t g_mpuSandboxInput
    __attribute__((section(".mpu_sandbox_input"), aligned(256)));
static MpuSandboxOutput_t g_mpuSandboxOutput
    __attribute__((section(".mpu_sandbox_output"), aligned(256)));
static uint8_t g_mpuSandboxArena[MPU_SANDBOX_ARENA_SIZE]
    __attribute__((section(".mpu_sandbox_arena"), aligned(256)));

static const TaskParameters_t g_mpuSandboxTaskParameters = {
    .pvTaskCode = MpuSandboxTask,
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
            .pvBaseAddress = &g_mpuSandboxOutput,
            .ulLengthInBytes = sizeof(g_mpuSandboxOutput),
            .ulParameters = portMPU_REGION_READ_WRITE | portMPU_REGION_EXECUTE_NEVER,
        },
        {
            .pvBaseAddress = g_mpuSandboxArena,
            .ulLengthInBytes = sizeof(g_mpuSandboxArena),
            .ulParameters = portMPU_REGION_READ_WRITE | portMPU_REGION_EXECUTE_NEVER,
        },
    },
    .pxTaskBuffer = &g_mpuSandboxTaskBuffer,
};

bool MpuSandboxTaskCreate(void)
{
    if (g_mpuSandboxTaskHandle != NULL) {
        return true;
    }

    memset(&g_mpuSandboxInput, 0, sizeof(g_mpuSandboxInput));
    memset(&g_mpuSandboxOutput, 0, sizeof(g_mpuSandboxOutput));
    memset(g_mpuSandboxArena, 0, sizeof(g_mpuSandboxArena));

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
    if (g_mpuSandboxInput.request.operation != 0) {
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
    g_mpuSandboxOutput.response.request_id = id;
    g_mpuSandboxOutput.response.status = MPU_SANDBOX_STATUS_PENDING;
    g_mpuSandboxOutput.response.output_length = 0U;
    g_mpuSandboxInput.request.request_id = id;
    g_mpuSandboxInput.request.input_length = (uint32_t) input_length;
    g_mpuSandboxInput.request.fault_address = 0U;
    if (operation == MPU_SANDBOX_OP_FAULT_OTHER_STACK) {
        g_mpuSandboxInput.request.fault_address = (uintptr_t) vTaskStackAddr();
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

    if ((response == NULL) || (g_mpuSandboxOutput.response.request_id != request_id) ||
            (g_mpuSandboxOutput.response.status == MPU_SANDBOX_STATUS_PENDING)) {
        return false;
    }

    taskENTER_CRITICAL();
    __asm volatile("dmb" ::: "memory");
    output_length = g_mpuSandboxOutput.response.output_length;
    if ((output_length > output_capacity) || ((output_length > 0U) && (output == NULL))) {
        taskEXIT_CRITICAL();
        return false;
    }
    *response = g_mpuSandboxOutput.response;
    if (output_length > 0U) {
        memcpy(output, g_mpuSandboxOutput.output, output_length);
    }
    g_mpuSandboxInput.request.operation = 0;
    taskEXIT_CRITICAL();
    return true;
}

static void MpuSandboxTask(void *argument)
{
    uint32_t notification;

    (void) argument;
    for (;;) {
        if (xTaskNotifyWait(0U, UINT32_MAX, &notification, portMAX_DELAY) == pdPASS) {
            (void) notification;
            MpuSandboxProcessRequest(&g_mpuSandboxInput, &g_mpuSandboxOutput, g_mpuSandboxArena);
        }
    }
}

static void MpuSandboxProcessRequest(MpuSandboxInput_t *input, MpuSandboxOutput_t *output,
                                     uint8_t *arena)
{
    MpuSandboxOperation_t operation = input->request.operation;
    uint32_t input_length = input->request.input_length;
    struct sha256 digest;

    output->response.output_length = 0U;
    if (input_length > sizeof(input->input)) {
        output->response.status = MPU_SANDBOX_STATUS_INVALID_REQUEST;
        return;
    }

    switch (operation) {
    case MPU_SANDBOX_OP_PING:
        output->output[0] = 'P';
        output->output[1] = 'O';
        output->output[2] = 'N';
        output->output[3] = 'G';
        output->response.output_length = 4U;
        __asm volatile("dmb" ::: "memory");
        output->response.status = MPU_SANDBOX_STATUS_OK;
        break;
    case MPU_SANDBOX_OP_ECHO:
        memcpy(arena, input->input, input_length);
        memcpy(output->output, arena, input_length);
        output->response.output_length = input_length;
        __asm volatile("dmb" ::: "memory");
        output->response.status = MPU_SANDBOX_STATUS_OK;
        break;
    case MPU_SANDBOX_OP_HASH:
        sha256(&digest, input->input, input_length);
        memcpy(output->output, digest.u.u8, sizeof(digest.u.u8));
        memset(&digest, 0, sizeof(digest));
        output->response.output_length = sizeof(digest.u.u8);
        __asm volatile("dmb" ::: "memory");
        output->response.status = MPU_SANDBOX_STATUS_OK;
        break;
    case MPU_SANDBOX_OP_FAULT_INPUT_WRITE:
    case MPU_SANDBOX_OP_FAULT_SHARED_HEAP:
    case MPU_SANDBOX_OP_FAULT_OTHER_STACK:
    case MPU_SANDBOX_OP_FAULT_PRIVILEGED_RAM:
    case MPU_SANDBOX_OP_FAULT_PERIPHERAL:
    case MPU_SANDBOX_OP_FAULT_SYSTEM_CONTROL:
    case MPU_SANDBOX_OP_FAULT_EXECUTE_NEVER:
    case MPU_SANDBOX_OP_HANG:
        MpuSandboxTriggerFault(operation, input, output, arena);
        break;
    default:
        output->response.status = MPU_SANDBOX_STATUS_INVALID_REQUEST;
        break;
    }
}

static void MpuSandboxTriggerFault(MpuSandboxOperation_t operation, MpuSandboxInput_t *input,
                                   MpuSandboxOutput_t *output, uint8_t *arena)
{
    volatile uint32_t *target;
    void (*execute)(void);

    switch (operation) {
    case MPU_SANDBOX_OP_FAULT_INPUT_WRITE:
        target = (volatile uint32_t *) input->input;
        *target = 0U;
        break;
    case MPU_SANDBOX_OP_FAULT_SHARED_HEAP:
        g_mpuSandboxSharedHeapProbe = 1U;
        break;
    case MPU_SANDBOX_OP_FAULT_OTHER_STACK:
        target = (volatile uint32_t *) input->request.fault_address;
        *target = 0U;
        break;
    case MPU_SANDBOX_OP_FAULT_PRIVILEGED_RAM:
        g_mpuSandboxPrivilegedProbe = 1U;
        break;
    case MPU_SANDBOX_OP_FAULT_PERIPHERAL:
        target = (volatile uint32_t *) 0x40000000UL;
        *target = 0U;
        break;
    case MPU_SANDBOX_OP_FAULT_SYSTEM_CONTROL:
        target = (volatile uint32_t *) 0xE000ED94UL;
        *target = 0U;
        break;
    case MPU_SANDBOX_OP_FAULT_EXECUTE_NEVER:
        arena[0] = 0x70U;
        arena[1] = 0x47U;
        __asm volatile("dsb\nisb" ::: "memory");
        execute = (void (*)(void))((uintptr_t) arena | 1U);
        execute();
        break;
    case MPU_SANDBOX_OP_HANG:
        for (;;) {
        }
        break;
    default:
        break;
    }

    output->response.status = MPU_SANDBOX_STATUS_INVALID_REQUEST;
}
