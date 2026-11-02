#include "password_test.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "keystore.h"

void PasswordTestFunc(int argc, char *argv[])
{
    unsigned int accountIndex = 0;
    if (argc != 3 || sscanf(argv[1], "%u", &accountIndex) != 1 || accountIndex > 2) {
        printf("input err!\r\n");
        return;
    }
    if (strcmp(argv[0], "verify") != 0) {
        printf("input err!\r\n");
        return;
    }

    int32_t ret = VerifyAccountPassword((uint8_t)accountIndex, argv[2]);
    printf("VerifyAccountPassword=%ld\r\n", (long)ret);
    printf("accountIndex=%u\r\n", accountIndex);
}
