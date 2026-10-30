#include "seed_phrase_test.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "account_manager.h"
#include "account_public_info.h"
#include "bip39.h"
#include "err_code.h"
#include "keystore.h"
#include "librust_c.h"
#include "slip39.h"
#include "user_utils.h"

static void DecodeWords(char *encoded)
{
    if (encoded == NULL) {
        return;
    }
    for (char *p = encoded; *p != '\0'; p++) {
        if (*p == '_') {
            *p = ' ';
        }
    }
}

static uint8_t WordCount(const char *words)
{
    if (words == NULL || words[0] == '\0') {
        return 0;
    }
    uint8_t count = 1;
    for (const char *p = words; *p != '\0'; p++) {
        if (*p == ' ') {
            count++;
        }
    }
    return count;
}

static void CheckBip39(uint8_t expectedWordCount, char *mnemonic)
{
    uint8_t seed[64] = {0};
    int32_t ret = ERR_GENERAL_FAIL;
    SimpleResponse_c_char *xpubResult = NULL;
    const char *status = "error";

    if (PassphraseExist(GetCurrentAccountIndex())) {
        printf("type=bip39\r\nstatus=passphrase_blocked\r\nSeedPhraseCheck=%d\r\n", ERR_GENERAL_FAIL);
        return;
    }
    if (WordCount(mnemonic) != expectedWordCount) {
        printf("type=bip39\r\nstatus=invalid\r\nSeedPhraseCheck=%d\r\n", ERR_KEYSTORE_MNEMONIC_INVALID);
        return;
    }

    do {
        ret = bip39_mnemonic_validate(NULL, mnemonic);
        if (ret != SUCCESS_CODE) {
            ret = ERR_KEYSTORE_MNEMONIC_INVALID;
            status = "invalid";
            break;
        }
        ret = bip39_mnemonic_to_seed(mnemonic, NULL, seed, sizeof(seed), NULL);
        if (ret != SUCCESS_CODE) {
            break;
        }
        // Keep this path identical to ModelBip39VerifyMnemonic.
        xpubResult = get_extended_pubkey_by_seed(seed, sizeof(seed), "M/49'/0'/0'");
        if (xpubResult == NULL || xpubResult->error_code != SUCCESS_CODE || xpubResult->data == NULL) {
            ret = xpubResult != NULL ? xpubResult->error_code : ERR_GENERAL_FAIL;
            break;
        }
        char *currentXpub = GetCurrentAccountPublicKey(XPUB_TYPE_BTC);
        if (currentXpub != NULL && strcmp(currentXpub, xpubResult->data) == 0) {
            ret = SUCCESS_CODE;
            status = "match";
        } else {
            ret = ERR_KEYSTORE_MNEMONIC_NOT_MATCH_WALLET;
            status = "not_match";
        }
    } while (0);

    CLEAR_ARRAY(seed);
    if (xpubResult != NULL) {
        free_simple_response_c_char(xpubResult);
    }
    printf("type=bip39\r\nstatus=%s\r\nSeedPhraseCheck=%ld\r\n", status, (long)ret);
}

static void CheckSlip39(uint8_t expectedWordCount, char *share)
{
    uint8_t threshold = 0;
    int32_t ret = ERR_GENERAL_FAIL;
    const char *status = "error";

    if (PassphraseExist(GetCurrentAccountIndex())) {
        printf("type=slip39\r\nstatus=passphrase_blocked\r\nSeedPhraseCheck=%d\r\n", ERR_GENERAL_FAIL);
        return;
    }
    if ((expectedWordCount != 20 && expectedWordCount != 33) || WordCount(share) != expectedWordCount) {
        printf("type=slip39\r\nstatus=invalid\r\nSeedPhraseCheck=%d\r\n", ERR_KEYSTORE_MNEMONIC_INVALID);
        return;
    }

    // Seed-check UI validates one share against this wallet's SLIP39 metadata.
    ret = Slip39OneSliceCheck(share, expectedWordCount, GetSlip39Id(), GetSlip39Eb(), GetSlip39Ie(), &threshold);
    int32_t rawRet = ret;
    if (ret >= 0) {
        status = "match";
        ret = SUCCESS_CODE;
    } else if (ret == SLIP39_NOT_BELONG_THIS_WALLET) {
        status = "not_match";
    } else {
        status = "invalid";
    }
    printf("type=slip39\r\nstatus=%s\r\nthreshold=%d\r\nSlip39CheckResult=%ld\r\nSeedPhraseCheck=%ld\r\n",
           status, threshold, (long)rawRet, (long)ret);
}

void SeedPhraseTestFunc(int argc, char *argv[])
{
    if (argc != 3) {
        printf("input err!\r\n");
        return;
    }
    int wordCount = 0;
    if (sscanf(argv[1], "%d", &wordCount) != 1 || wordCount <= 0 || wordCount > UINT8_MAX) {
        printf("status=invalid_word_count\r\nSeedPhraseCheck=%d\r\n", ERR_KEYSTORE_MNEMONIC_INVALID);
        return;
    }
    DecodeWords(argv[2]);
    if (strcmp(argv[0], "bip39") == 0) {
        CheckBip39((uint8_t)wordCount, argv[2]);
    } else if (strcmp(argv[0], "slip39") == 0) {
        CheckSlip39((uint8_t)wordCount, argv[2]);
    } else {
        printf("status=unsupported_type\r\nSeedPhraseCheck=%d\r\n", ERR_KEYSTORE_MNEMONIC_INVALID);
    }
}
