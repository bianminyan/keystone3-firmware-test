"""Host regression for the account guard in the Zcash scan result page."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ZcashScanAccountGuardTest(unittest.TestCase):
    def test_account_guard_before_key_access(self):
        source = (ROOT / 'src/ui/gui_widgets/gui_scan_widgets.c').read_text()
        start = source.index('static void GuiScanShowZcashAddress(const char *address)')
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
static uint8_t slot;
static int reads, support_checks, key_reads, verifies, observed;
static bool supported, key_available;
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
                GuiScanShowZcashAddress("u1example");
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


class ZcashUiLifetimeTest(unittest.TestCase):
    def compile_and_run(self, harness):
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / 'regression.c'
            binary = Path(tmp) / 'regression'
            source.write_text(harness)
            subprocess.run(['cc', '-std=gnu11', '-fsanitize=address,undefined',
                            str(source), '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)

    def test_scan_message_survives_rust_result_release(self):
        source = (ROOT / 'src/ui/gui_analyze/gui_resolve_ur.c').read_text()
        start = source.index('    case ZcashAddressCheck: {')
        end = source.index('    case ZcashBatchTx:', start)
        case = source[start:end]
        header = (ROOT / 'src/ui/gui_widgets/gui_scan_widgets.h').read_text()
        start = header.index('typedef struct {')
        end = header.index('} ZcashAddressScanResult_t;', start) + len('} ZcashAddressScanResult_t;')
        payload = header[start:end]
        self.compile_and_run(r'''
#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
typedef struct { uint8_t viewType, urType; } UrViewType_t;
typedef struct { char *data; } URParseResult;
enum { ZcashAddressCheck = 42, SIG_QRCODE_VIEW_SCAN_PASS = 99 };
''' + payload + r'''
static ZcashAddressScanResult_t queued;
static int freed, reject_message, stopped, sent;
#define EOK 0
#define SUCCESS_CODE 0
static int strcpy_s(char *dest, size_t size, const char *source) {
    size_t length = strlen(source);
    if (length >= size) { dest[0] = 0; return 1; }
    memcpy(dest, source, length + 1);
    return EOK;
}
static const char *zcash_address_qr_text(char *data) { return data; }
static void free_ur_parse_result(URParseResult *result) {
    free(result->data); free(result); freed++;
}
static void StopQrDecode(void) { stopped++; }
static int GuiApiEmitSignal(int signal, void *data, unsigned size) {
    sent++;
    assert(stopped == 0);
    assert(freed == 1);
    assert(signal == SIG_QRCODE_VIEW_SCAN_PASS);
    assert(size == sizeof(queued));
    if (reject_message) return -1;
    memcpy(&queued, data, size);
    return 0;
}
static void handle(URParseResult *urResult, UrViewType_t urViewType) {
    switch (urViewType.viewType) {
''' + case + r'''
    }
}
int main(void) {
    for (int reject = 0; reject <= 1; reject++) {
        reject_message = reject;
        for (int length = 2; length <= 1026; length++) {
            URParseResult *result = malloc(sizeof(*result));
            result->data = malloc(length + 1);
            memset(result->data, 'Q', length);
            result->data[0] = 'U'; result->data[1] = '1'; result->data[length] = 0;
            freed = stopped = sent = 0;
            handle(result, (UrViewType_t){ZcashAddressCheck, 7});
            assert(freed == 1);
            assert(sent == (length <= 1024));
            assert(stopped == (!reject && length <= 1024));
            if (!reject && length <= 1024) {
                assert(queued.viewType.viewType == ZcashAddressCheck);
                assert(strlen(queued.address) == length);
                assert(queued.address[0] == 'U' && queued.address[1] == '1');
                for (int i = 2; i < length; i++) assert(queued.address[i] == 'Q');
            }
        }
    }
    URParseResult *missing = malloc(sizeof(*missing));
    missing->data = NULL;
    freed = stopped = sent = 0;
    handle(missing, (UrViewType_t){ZcashAddressCheck, 7});
    assert(freed == 1 && stopped == 0 && sent == 0);
}
''')

    def test_optional_hint_labels_are_not_dereferenced(self):
        source = (ROOT / 'src/ui/gui_components/gui_hintbox.c').read_text()
        start = source.index('    uint32_t height =', source.index('void *GuiCreateGeneralHintBox'))
        end = source.index('    GuiHintBoxResize(cont, height);', start)
        calculation = source[start:end]
        self.compile_and_run(r'''
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
static int lv_obj_get_self_height(int *obj) { assert(obj); return *obj; }
static int lv_obj_get_self_width(int *obj) { assert(obj); return *obj; }
int main(void) {
    int label = 20;
    for (int mask = 0; mask < 8; mask++) {
        int *title = &label;
        int *desc1 = (mask & 1) ? &label : NULL;
        int *desc2 = (mask & 2) ? &label : NULL;
        int *img = (mask & 4) ? &label : NULL;
''' + calculation + r'''
        assert(height == 174 + (desc1 ? 32 : 0) + (desc2 ? 32 : 0) + (img ? 68 : 0));
    }
}
''')


if __name__ == '__main__':
    unittest.main()
