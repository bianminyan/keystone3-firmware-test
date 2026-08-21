#ifndef MPU_SANDBOX_TASK_H
#define MPU_SANDBOX_TASK_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef enum {
    MPU_SANDBOX_OP_PING = 1,
    MPU_SANDBOX_OP_ECHO,
    MPU_SANDBOX_OP_HASH,
    MPU_SANDBOX_OP_FAULT_INPUT_WRITE,
    MPU_SANDBOX_OP_FAULT_SHARED_HEAP,
    MPU_SANDBOX_OP_FAULT_OTHER_STACK,
    MPU_SANDBOX_OP_FAULT_PRIVILEGED_RAM,
    MPU_SANDBOX_OP_FAULT_PERIPHERAL,
    MPU_SANDBOX_OP_FAULT_SYSTEM_CONTROL,
    MPU_SANDBOX_OP_FAULT_EXECUTE_NEVER,
    MPU_SANDBOX_OP_HANG,
} MpuSandboxOperation_t;

typedef enum {
    MPU_SANDBOX_STATUS_IDLE = 0,
    MPU_SANDBOX_STATUS_PENDING,
    MPU_SANDBOX_STATUS_OK,
    MPU_SANDBOX_STATUS_INVALID_REQUEST,
    MPU_SANDBOX_STATUS_OUTPUT_TOO_SMALL,
} MpuSandboxStatus_t;

typedef struct {
    uint32_t request_id;
    MpuSandboxOperation_t operation;
    uint32_t input_length;
    uintptr_t fault_address;
} MpuSandboxRequest_t;

typedef struct {
    uint32_t request_id;
    MpuSandboxStatus_t status;
    uint32_t output_length;
} MpuSandboxResponse_t;

bool MpuSandboxTaskCreate(void);
bool MpuSandboxSubmit(MpuSandboxOperation_t operation, const uint8_t *input, size_t input_length,
                      uint32_t *request_id);
bool MpuSandboxGetResponse(uint32_t request_id, MpuSandboxResponse_t *response, uint8_t *output,
                           size_t output_capacity);

#endif
