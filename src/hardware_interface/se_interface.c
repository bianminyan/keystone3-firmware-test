#include "se_interface.h"
#include "drv_atecc608b.h"
#include "drv_ds28s60.h"
#include "user_utils.h"
#include "cryptoauthlib.h"
#include "assert.h"
#include "drv_trng.h"
#include "drv_mpu.h"

//START: Atecc608b
int32_t SE_EncryptWrite(uint8_t slot, uint8_t block, const uint8_t *data)
{
    int32_t ret = Atecc608bEncryptWrite(slot, block, data);
    ASSERT(ret == ATCA_SUCCESS);
    return SUCCESS_CODE;
}

int32_t SE_Kdf(uint8_t slot, const uint8_t *authKey, const uint8_t *inData, uint32_t inLen, uint8_t *outData)
{
    int32_t ret = Atecc608bKdf(slot, authKey, inData, inLen, outData);
    if (ret == ATCA_CHECKMAC_VERIFY_FAILED) {
        return ret;
    }
    ASSERT(ret == ATCA_SUCCESS);
    return SUCCESS_CODE;
}

int32_t SE_DeriveKey(uint8_t slot, const uint8_t *authKey)
{
    int32_t ret = Atecc608bDeriveKey(slot, authKey);
    ASSERT(ret == ATCA_SUCCESS);
    return SUCCESS_CODE;
}
//END

//START: DS28S60
// Authenticated by default: every DS28S60 page read is bound to the device's true page content via a
// CRPA (0xA5) over a fresh host nonce (see DS28S60_HmacAuthenticatedRead).
// CONSTRAINT: every page read through here MUST be CRPA-able — a user page <= 91 (blocks 0..22, bound
// to Secret A / APH+EPH by production provisioning; 
int32_t SE_HmacEncryptRead(uint8_t *data, uint8_t page)
{
    // Enforce the CRPA-able range in code, not just the comment above: a read outside the Secret-A
    // user-page range (0..MAX_USER_PAGE) would fail the CRPA and reboot with a misleading "tamper"
    // cause. Assert here so an out-of-range read faults immediately at the offending call site.
    ASSERT(page <= MAX_USER_PAGE);
    int32_t ret = DS28S60_HmacAuthenticatedRead(data, page);
    ASSERT(ret == DS28S60_SUCCESS);
    return SUCCESS_CODE;
}

int32_t SE_GetDS28S60Rng(uint8_t *rngArray, uint32_t num)
{
    int32_t ret = DS28S60_GetRng(rngArray, num) ;
    ASSERT(ret == DS28S60_SUCCESS);
    return SUCCESS_CODE;
}

void SE_GetTRng(void *buf, uint32_t len)
{
    return TrngGet(buf, len);
}

int32_t SE_GetAtecc608bRng(uint8_t *rngArray, uint32_t num)
{
    int32_t ret = Atecc608bGetRng(rngArray, num);
    ASSERT(ret == ATCA_SUCCESS);
    return SUCCESS_CODE;
}

int32_t SE_HmacEncryptWrite(const uint8_t *data, uint8_t page)
{
    int32_t ret = DS28S60_HmacEncryptWrite(data, page);
    ASSERT(ret == DS28S60_SUCCESS);
    return SUCCESS_CODE;
}

//END
