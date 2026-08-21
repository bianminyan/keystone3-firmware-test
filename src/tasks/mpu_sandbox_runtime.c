#include "mpu_sandbox_internal.h"

#include "FreeRTOS.h"
#include "task.h"

static void MpuSandboxProcessRequest(MpuSandboxInput_t *input, MpuSandboxRw_t *rw);
static void MpuSandboxTriggerFault(MpuSandboxOperation_t operation, MpuSandboxInput_t *input,
                                   MpuSandboxOutput_t *output);
static void MpuSandboxOverflowOwnStack(uint32_t depth);

static void SandboxCopy(void *destination, const void *source, size_t length)
{
    uint8_t *destinationBytes = destination;
    const uint8_t *sourceBytes = source;

    for (size_t i = 0U; i < length; i++) {
        destinationBytes[i] = sourceBytes[i];
    }
}

static void SandboxZero(void *destination, size_t length)
{
    volatile uint8_t *destinationBytes = destination;

    for (size_t i = 0U; i < length; i++) {
        destinationBytes[i] = 0U;
    }
}

void MpuSandboxRuntime(void *argument)
{
    uint32_t notification;

    (void)argument;
    for (;;) {
        if (xTaskNotifyWait(0U, UINT32_MAX, &notification, portMAX_DELAY) == pdPASS) {
            (void)notification;
            MpuSandboxProcessRequest(&g_mpuSandboxInput, &g_mpuSandboxRw);
        }
    }
}

static void MpuSandboxProcessRequest(MpuSandboxInput_t *input, MpuSandboxRw_t *rw)
{
    MpuSandboxOperation_t operation = input->request.operation;
    uint32_t inputLength = input->request.input_length;
    MpuSandboxOutput_t *output = &rw->output;

    output->response.output_length = 0U;
    if (inputLength > sizeof(input->input)) {
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
        SandboxCopy(output->output, input->input, inputLength);
        output->response.output_length = inputLength;
        __asm volatile("dmb" ::: "memory");
        output->response.status = MPU_SANDBOX_STATUS_OK;
        break;
    case MPU_SANDBOX_OP_FAULT_INPUT_WRITE:
    case MPU_SANDBOX_OP_FAULT_SHARED_HEAP:
    case MPU_SANDBOX_OP_FAULT_OTHER_STACK:
    case MPU_SANDBOX_OP_FAULT_STACK_OVERFLOW:
    case MPU_SANDBOX_OP_FAULT_PRIVILEGED_RAM:
    case MPU_SANDBOX_OP_FAULT_PERIPHERAL:
    case MPU_SANDBOX_OP_FAULT_SYSTEM_CONTROL:
    case MPU_SANDBOX_OP_FAULT_GENERAL_FLASH:
    case MPU_SANDBOX_OP_FAULT_EXECUTE_NEVER:
    case MPU_SANDBOX_OP_FAULT_FORBIDDEN_SYSCALL:
    case MPU_SANDBOX_OP_HANG:
        MpuSandboxTriggerFault(operation, input, output);
        break;
#ifndef BTC_ONLY
    case MPU_SANDBOX_OP_EAPDU_FRAME: {
        EapduFramingResult_t framing = EapduFramingPush(&rw->eapdu, input->input,
                                                        inputLength,
                                                        (uint32_t)input->request.fault_address);
        MpuSandboxEapduResult_t result = {
            .framing_status = (uint32_t)framing.status,
            .timed_out = framing.timed_out ? 1U : 0U,
            .payload_length = framing.payload_length,
            .command_type = framing.command_type,
            .request_id = framing.request_id,
            .total_packets = framing.total_packets,
            .packet_index = framing.packet_index,
            .first_missing_packet = framing.first_missing_packet,
            .cla = framing.cla,
        };
        SandboxCopy(output->output, &result, sizeof(result));
        output->response.output_length = sizeof(result);
        __asm volatile("dmb" ::: "memory");
        output->response.status = MPU_SANDBOX_STATUS_OK;
        break;
    }
#endif
    default:
        output->response.status = MPU_SANDBOX_STATUS_INVALID_REQUEST;
        break;
    }
}

static void MpuSandboxTriggerFault(MpuSandboxOperation_t operation, MpuSandboxInput_t *input,
                                   MpuSandboxOutput_t *output)
{
    volatile uint32_t *target;
    void (*execute)(void);

    switch (operation) {
    case MPU_SANDBOX_OP_FAULT_INPUT_WRITE:
        target = (volatile uint32_t *)input->input;
        *target = 0U;
        break;
    case MPU_SANDBOX_OP_FAULT_SHARED_HEAP:
        g_mpuSandboxSharedHeapProbe = 1U;
        break;
    case MPU_SANDBOX_OP_FAULT_OTHER_STACK:
        target = (volatile uint32_t *)input->request.fault_address;
        *target = 0U;
        break;
    case MPU_SANDBOX_OP_FAULT_STACK_OVERFLOW:
        MpuSandboxOverflowOwnStack(0U);
        break;
    case MPU_SANDBOX_OP_FAULT_PRIVILEGED_RAM:
        g_mpuSandboxPrivilegedProbe = 1U;
        break;
    case MPU_SANDBOX_OP_FAULT_PERIPHERAL:
        target = (volatile uint32_t *)0x40000000UL;
        *target = 0U;
        break;
    case MPU_SANDBOX_OP_FAULT_SYSTEM_CONTROL:
        target = (volatile uint32_t *)0xE000ED94UL;
        *target = 0U;
        break;
    case MPU_SANDBOX_OP_FAULT_GENERAL_FLASH:
        target = (volatile uint32_t *)input->request.fault_address;
        output->response.output_length = *target;
        break;
    case MPU_SANDBOX_OP_FAULT_EXECUTE_NEVER:
        output->output[0] = 0x70U;
        output->output[1] = 0x47U;
        __asm volatile("dsb\nisb" ::: "memory");
        execute = (void (*)(void))((uintptr_t)output->output | 1U);
        execute();
        break;
    case MPU_SANDBOX_OP_FAULT_FORBIDDEN_SYSCALL:
        (void)xTaskGetTickCount();
        break;
    case MPU_SANDBOX_OP_HANG:
        while (1) {
        }
        break;
    default:
        break;
    }

    output->response.status = MPU_SANDBOX_STATUS_INVALID_REQUEST;
}

__attribute__((noinline)) static void MpuSandboxOverflowOwnStack(uint32_t depth)
{
    volatile uint8_t frame[192];

    for (size_t i = 0U; i < sizeof(frame); i++) {
        frame[i] = (uint8_t)(depth + i);
    }
    __asm volatile("" : : "r"(frame) : "memory");
    MpuSandboxOverflowOwnStack(depth + 1U);
}
