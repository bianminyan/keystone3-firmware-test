"""Host regression for the account guard in the Zcash scan result page."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ZcashScanAccountGuardTest(unittest.TestCase):
    def test_account_guard_before_key_access(self):
        source = (ROOT / 'src/ui/gui_widgets/gui_scan_widgets.c').read_text()
        start = source.index('static void GuiScanShowZcashAddress(void)')
        end = source.index('    CLEAR_ARRAY(ufvk);', start)
        function = source[start:end] + '    observed = result;\n}\n'
        harness = r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#define ZCASH_UFVK_BUFFER_SIZE 256
#define SUCCESS_CODE 0
#define ZcashAddressKeyUnavailable 0
#define ZcashAddressOwned 1
typedef int ZcashAddressVerification;
static struct { void *data; } qr;
static void *g_pageWidget = &qr;
static typeof(qr) *g_zcashAddressQr = &qr;
static uint8_t slot;
static int reads, support_checks, key_reads, verifies, observed;
static bool supported, key_available;
static void ClearZcashAddressQr(void) {}
static const char *zcash_address_qr_text(void *p) { return "u1example"; }
static uint8_t GetCurrentAccountIndex(void) { reads++; return slot; }
static bool IsZcashSupportedForCurrentMnemonic(void) { support_checks++; return supported; }
static int GetZcashUFVK(uint8_t index, char *out) {
    assert(index <= 2);
    key_reads++;
    if (key_available) strcpy(out, "key");
    return SUCCESS_CODE;
}
static int verify_zcash_address(char *key, char *address) { verifies++; return ZcashAddressOwned; }
'''
        harness += function + r'''
int main(void) {
    for (int index = 0; index <= 255; index++) {
        for (int support = 0; support <= 1; support++) {
            for (int available = 0; available <= 1; available++) {
                slot = index; supported = support; key_available = available;
                reads = support_checks = key_reads = verifies = 0;
                GuiScanShowZcashAddress();
                assert(reads == 1);
                assert(support_checks == (index <= 2));
                assert(key_reads == (index <= 2 && support));
                assert(verifies == (index <= 2 && support && available));
                assert(observed == (verifies ? ZcashAddressOwned : ZcashAddressKeyUnavailable));
            }
        }
    }
    return 0;
}
'''
        with tempfile.TemporaryDirectory() as tmp:
            source_path = Path(tmp) / 'scan_guard.c'
            executable = Path(tmp) / 'scan_guard'
            source_path.write_text(harness)
            subprocess.run(['cc', '-std=gnu11', str(source_path), '-o', str(executable)], check=True)
            subprocess.run([str(executable)], check=True)


if __name__ == '__main__':
    unittest.main()
