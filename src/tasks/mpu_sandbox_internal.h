#ifndef MPU_SANDBOX_INTERNAL_H
#define MPU_SANDBOX_INTERNAL_H

#include "mpu_sandbox_task.h"

#ifndef BTC_ONLY
#include "eapdu_framing.h"
#endif

#define MPU_SANDBOX_STACK_WORDS     1024U
#define MPU_SANDBOX_INPUT_SIZE      240U
#define MPU_SANDBOX_OUTPUT_SIZE     244U
#define MPU_SANDBOX_RW_REGION_SIZE  16384U
#define MPU_SANDBOX_TEXT_REGION_SIZE 16384U

typedef struct {
    volatile MpuSandboxRequest_t request;
    uint8_t input[MPU_SANDBOX_INPUT_SIZE];
} MpuSandboxInput_t;

typedef struct {
    volatile MpuSandboxResponse_t response;
    uint8_t output[MPU_SANDBOX_OUTPUT_SIZE];
} MpuSandboxOutput_t;

typedef struct {
    MpuSandboxOutput_t output;
#ifndef BTC_ONLY
    EapduFramingState_t eapdu;
#endif
} MpuSandboxRw_t;

_Static_assert(sizeof(MpuSandboxInput_t) == 256U,
               "sandbox input must occupy one MPU region");
_Static_assert(sizeof(MpuSandboxOutput_t) == 256U,
               "sandbox output header must remain fixed");
_Static_assert(sizeof(MpuSandboxRw_t) <= MPU_SANDBOX_RW_REGION_SIZE,
               "sandbox writable state must fit its MPU region");

extern MpuSandboxInput_t g_mpuSandboxInput;
extern MpuSandboxRw_t g_mpuSandboxRw;
extern volatile uint32_t g_mpuSandboxPrivilegedProbe;
extern volatile uint8_t g_mpuSandboxSharedHeapProbe;

void MpuSandboxRuntime(void *argument);

#endif
