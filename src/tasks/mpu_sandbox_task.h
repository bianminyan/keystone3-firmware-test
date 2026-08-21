#ifndef MPU_SANDBOX_TASK_H
#define MPU_SANDBOX_TASK_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef enum {
    MPU_SANDBOX_OP_PING = 1,
    MPU_SANDBOX_OP_ECHO,
    MPU_SANDBOX_OP_FAULT_INPUT_WRITE,
    MPU_SANDBOX_OP_FAULT_SHARED_HEAP,
    MPU_SANDBOX_OP_FAULT_OTHER_STACK,
    MPU_SANDBOX_OP_FAULT_STACK_OVERFLOW,
    MPU_SANDBOX_OP_FAULT_PRIVILEGED_RAM,
    MPU_SANDBOX_OP_FAULT_PERIPHERAL,
    MPU_SANDBOX_OP_FAULT_SYSTEM_CONTROL,
    MPU_SANDBOX_OP_FAULT_GENERAL_FLASH,
    MPU_SANDBOX_OP_FAULT_EXECUTE_NEVER,
    MPU_SANDBOX_OP_FAULT_FORBIDDEN_SYSCALL,
    MPU_SANDBOX_OP_HANG,
    MPU_SANDBOX_OP_EAPDU_FRAME,
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

#ifndef BTC_ONLY
typedef struct {
    uint32_t framing_status;
    uint32_t timed_out;
    uint32_t payload_length;
    uint16_t command_type;
    uint16_t request_id;
    uint16_t total_packets;
    uint16_t packet_index;
    uint16_t first_missing_packet;
    uint8_t cla;
    uint8_t reserved;
} MpuSandboxEapduResult_t;
#endif

bool MpuSandboxTaskCreate(void);
bool MpuSandboxSubmit(MpuSandboxOperation_t operation, const uint8_t *input, size_t input_length,
                      uint32_t *request_id);
bool MpuSandboxGetResponse(uint32_t request_id, MpuSandboxResponse_t *response, uint8_t *output,
                           size_t output_capacity);

#ifndef BTC_ONLY
bool MpuSandboxGetEapduResult(uint32_t request_id, MpuSandboxEapduResult_t *result,
                              const uint8_t **payload);
bool MpuSandboxReleaseEapduResult(uint32_t request_id, bool reset_framing);
bool MpuSandboxResetEapduFraming(void);
#endif

#endif
