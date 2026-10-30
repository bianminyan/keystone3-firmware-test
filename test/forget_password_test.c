#include "forget_password_test.h"

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "account_manager.h"
#include "account_public_info.h"
#include "bip39.h"
#include "err_code.h"
#include "fingerprint_process.h"
#include "keystore.h"
#include "librust_c.h"
#include "se_manager.h"
#include "secret_cache.h"
#include "slip39.h"
#include "user_utils.h"

#define FORGET_PASSWORD_LOGIN_ATTEMPT_LIMIT 10

typedef struct {
    bool slip39;
    uint8_t entropy[32];
    uint8_t ems[32];
    uint8_t entropyLen;
    uint16_t identifier;
    bool extendable;
    uint8_t iterationExponent;
    uint8_t accountIndex;
} RecoveredWallet;

static void DecodeWords(char *encoded)
{
    for (char *p = encoded; p != NULL && *p != '\0'; p++) {
        if (*p == '_') {
            *p = ' ';
        }
    }
}

static int32_t FindAccountForSeed(uint8_t *seed, uint8_t seedLen, uint8_t *accountIndex)
{
    SimpleResponse_c_char *xpub = get_extended_pubkey_by_seed(seed, seedLen, "M/49'/0'/0'");
    if (xpub == NULL || xpub->error_code != SUCCESS_CODE || xpub->data == NULL) {
        int32_t ret = xpub != NULL ? xpub->error_code : ERR_GENERAL_FAIL;
        if (xpub != NULL) {
            free_simple_response_c_char(xpub);
        }
        return ret;
    }
    int32_t ret = FindAccountByXpub(xpub->data, accountIndex);
#ifdef COMPILE_SIMULATOR
    // The simulator does not persist every secure-element IV page used by
    // FindAccountByXpub's hardware existence check. Its public-info slots are
    // authoritative for the simulated account, so use the legacy lookup only
    // when the production lookup reports no match.
    if (ret == SUCCESS_CODE && *accountIndex == 0xFF) {
        *accountIndex = SpecifiedXPubExist(xpub->data);
    }
#endif
    free_simple_response_c_char(xpub);
    if (ret == SUCCESS_CODE && *accountIndex == 0xFF) {
        return ERR_KEYSTORE_MNEMONIC_NOT_MATCH_WALLET;
    }
    return ret;
}

static int32_t RecoverBip39(char *mnemonic, RecoveredWallet *wallet)
{
    uint8_t seed[64] = {0};
    size_t entropyLen = 0;
    int32_t ret = bip39_mnemonic_validate(NULL, mnemonic);
    if (ret != SUCCESS_CODE) {
        return ERR_KEYSTORE_MNEMONIC_INVALID;
    }
    ret = bip39_mnemonic_to_bytes(NULL, mnemonic, wallet->entropy, sizeof(wallet->entropy), &entropyLen);
    if (ret != SUCCESS_CODE || (entropyLen != 16 && entropyLen != 24 && entropyLen != 32)) {
        return ERR_KEYSTORE_MNEMONIC_INVALID;
    }
    ret = bip39_mnemonic_to_seed(mnemonic, NULL, seed, sizeof(seed), NULL);
    if (ret == SUCCESS_CODE) {
        ret = FindAccountForSeed(seed, sizeof(seed), &wallet->accountIndex);
    }
    CLEAR_ARRAY(seed);
    wallet->slip39 = false;
    wallet->entropyLen = (uint8_t)entropyLen;
    return ret;
}

static int32_t RecoverSlip39(uint8_t threshold, uint8_t wordCount, char *shares[], RecoveredWallet *wallet)
{
    uint8_t seed[64] = {0};
    int32_t ret = Slip39GetMasterSecret(
        threshold,
        wordCount,
        wallet->ems,
        wallet->entropy,
        shares,
        &wallet->identifier,
        &wallet->extendable,
        &wallet->iterationExponent);
    if (ret != SUCCESS_CODE) {
        return ERR_KEYSTORE_MNEMONIC_INVALID;
    }
    wallet->entropyLen = wordCount == 20 ? 16 : 32;
    ret = Slip39GetSeed(
        wallet->ems,
        seed,
        wallet->entropyLen,
        "",
        wallet->iterationExponent,
        wallet->extendable,
        wallet->identifier);
    if (ret == SUCCESS_CODE) {
        ret = FindAccountForSeed(seed, wallet->entropyLen, &wallet->accountIndex);
    }
    CLEAR_ARRAY(seed);
    wallet->slip39 = true;
    return ret;
}

static int32_t ResetRecoveredWallet(
    const RecoveredWallet *wallet,
    const char *newPassword,
    const char *ownershipPassword,
    const char **status,
    uint8_t *ownershipAccountIndex)
{
    AccountInfo_t previous = {0};
    bool cleanupRequired = false;
    uint8_t accountCount = 0;
    int32_t ret = GetAccountInfo(wallet->accountIndex, &previous);
    if (ret != SUCCESS_CODE) {
        *status = "account_info_error";
        return ret;
    }
    if (strnlen_s(newPassword, PASSWORD_MAX_LEN + 1) < 6) {
        *status = "new_password_invalid";
        return ERR_GENERAL_FAIL;
    }
    ret = CheckPasswordExisted(newPassword, wallet->accountIndex);
    if (ret != SUCCESS_CODE) {
        *status = "new_password_reused";
        return ret;
    }
    ret = GetExistAccountNum(&accountCount);
    if (ret != SUCCESS_CODE) {
        *status = "account_count_error";
        return ret;
    }
    if (GetSeGen() == SE_GEN_2 && accountCount > 1) {
        if (ownershipPassword == NULL || strcmp(ownershipPassword, "-") == 0) {
            *status = "ownership_required";
            return ERR_KEYSTORE_PASSWORD_ERR;
        }
        ret = VerifyOwnershipPasswordTryAll(
            ownershipAccountIndex,
            ownershipPassword,
            FORGET_PASSWORD_LOGIN_ATTEMPT_LIMIT);
        if (ret != SUCCESS_CODE || *ownershipAccountIndex == wallet->accountIndex) {
            *status = "ownership_error";
            return ret == SUCCESS_CODE ? ERR_KEYSTORE_PASSWORD_ERR : ret;
        }
        SE_ArmProvisionRecovery(*ownershipAccountIndex, ownershipPassword);
    }

    SecretCacheSetWalletName(previous.walletName);
    SecretCacheSetWalletIndex(previous.iconIndex);
    ret = wallet->slip39 ?
        CreateNewSlip39Account(
            wallet->accountIndex,
            wallet->ems,
            wallet->entropy,
            wallet->entropyLen,
            newPassword,
            wallet->identifier,
            wallet->extendable,
            wallet->iterationExponent,
            &cleanupRequired) :
        CreateNewAccount(
            wallet->accountIndex,
            wallet->entropy,
            wallet->entropyLen,
            newPassword,
            &cleanupRequired);
    if (ret == SUCCESS_CODE) {
        ClearAccountPassphrase(wallet->accountIndex);
        ret = SetWalletDesc(previous.walletName, previous.iconIndex);
    }
    if (ret == SUCCESS_CODE) {
        UpdateFingerSignFlag(wallet->accountIndex, false);
        ret = SE_SetAccountStatus(wallet->accountIndex, ACCOUNT_STATUS_CREATED);
    }
    if (ret == SUCCESS_CODE) {
        uint8_t loggedIn = 0xFF;
        ret = VerifyPasswordAndLogin(&loggedIn, newPassword);
        if (ret == SUCCESS_CODE && loggedIn != wallet->accountIndex) {
            ret = ERR_GENERAL_FAIL;
        }
    }
    if (ret != SUCCESS_CODE && cleanupRequired) {
        DestroyAccount(wallet->accountIndex);
    }
    LogoutCurrentAccount();
    AbandonProvisionRecovery();
    ClearSecretCache();
    CLEAR_OBJECT(previous);
    *status = ret == SUCCESS_CODE ? "success" : "reset_error";
    return ret;
}

static bool ForgetPasswordNeedsOwnership(uint8_t accountCount)
{
    return GetSeGen() == SE_GEN_2 && accountCount > 1;
}

static void PrintDeviceCapabilities(void)
{
    uint8_t accountCount = 0;
    int32_t ret = GetExistAccountNum(&accountCount);
    printf("seGeneration=%u\r\n", (unsigned int)GetSeGen());
    printf("accountCount=%u\r\n", accountCount);
    printf("ownershipRequired=%u\r\n", ret == SUCCESS_CODE && ForgetPasswordNeedsOwnership(accountCount));
    printf("ForgetPasswordInfo=%ld\r\n", (long)ret);
}

static void PrintResult(const char *type, const char *status, int32_t ret, uint8_t accountIndex, uint8_t ownerIndex)
{
    uint8_t accountCount = 0;
    GetExistAccountNum(&accountCount);
    printf("type=%s\r\n", type);
    printf("status=%s\r\n", status);
    printf("seGeneration=%u\r\n", (unsigned int)GetSeGen());
    printf("ownershipRequired=%u\r\n", ForgetPasswordNeedsOwnership(accountCount));
    printf("accountIndex=%u\r\n", accountIndex);
    printf("ownershipAccountIndex=%u\r\n", ownerIndex);
    printf("ForgetPasswordReset=%ld\r\n", (long)ret);
}

void ForgetPasswordTestFunc(int argc, char *argv[])
{
    RecoveredWallet wallet = {.accountIndex = 0xFF};
    uint8_t ownershipAccountIndex = 0xFF;
    const char *status = "input_error";
    int32_t ret = ERR_GENERAL_FAIL;

    if (argc == 1 && strcmp(argv[0], "info") == 0) {
        PrintDeviceCapabilities();
        return;
    }

    if (argc == 4 && strcmp(argv[0], "bip39") == 0) {
        DecodeWords(argv[3]);
        ret = RecoverBip39(argv[3], &wallet);
        if (ret == SUCCESS_CODE) {
            ret = ResetRecoveredWallet(&wallet, argv[1], argv[2], &status, &ownershipAccountIndex);
        } else {
            status = ret == ERR_KEYSTORE_MNEMONIC_INVALID ? "mnemonic_invalid" : "mnemonic_not_match";
        }
        PrintResult("bip39", status, ret, wallet.accountIndex, ownershipAccountIndex);
        CLEAR_OBJECT(wallet);
        return;
    }

    if (argc >= 6 && strcmp(argv[0], "slip39") == 0) {
        unsigned int threshold = 0;
        unsigned int wordCount = 0;
        if (sscanf(argv[1], "%u", &threshold) != 1 ||
                sscanf(argv[2], "%u", &wordCount) != 1 ||
                threshold == 0 || threshold > SLIP39_MAX_MEMBER ||
                argc != (int)(5 + threshold) ||
                (wordCount != 20 && wordCount != 33)) {
            PrintResult("slip39", status, ret, wallet.accountIndex, ownershipAccountIndex);
            return;
        }
        char *shares[SLIP39_MAX_MEMBER] = {0};
        for (unsigned int i = 0; i < threshold; i++) {
            DecodeWords(argv[5 + i]);
            shares[i] = argv[5 + i];
        }
        ret = RecoverSlip39((uint8_t)threshold, (uint8_t)wordCount, shares, &wallet);
        if (ret == SUCCESS_CODE) {
            ret = ResetRecoveredWallet(&wallet, argv[3], argv[4], &status, &ownershipAccountIndex);
        } else {
            status = ret == ERR_KEYSTORE_MNEMONIC_INVALID ? "mnemonic_invalid" : "mnemonic_not_match";
        }
        PrintResult("slip39", status, ret, wallet.accountIndex, ownershipAccountIndex);
        CLEAR_OBJECT(wallet);
        return;
    }

    printf("input err!\r\n");
}
