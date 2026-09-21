"""Regressions for attached assets remaining visible on TRON review screens."""
import json
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def nodes(node):
    yield node
    for child in node.get("children", []):
        yield from nodes(child)


class TronAttachedAssetTests(unittest.TestCase):
    def test_overview_and_details_expose_attached_assets_and_warning(self):
        source = (ROOT / "src/ui/gui_analyze/multi/web3/gui_general_analyze.h").read_text()
        templates = (ROOT / "src/ui/gui_analyze/multi/web3/gui_trx_template.h").read_text()
        for name in ("REMAPVIEW_TRX", "REMAPVIEW_TRX_SWAP"):
            macro = re.search(name + r',\\\n\s*(\w+)', source)[1]
            line = re.search(r"#define " + macro + r" ([^\n]+)", templates)[1]
            tree = json.loads(json.loads(re.search(r'"(?:\\.|[^"\\])*"', line)[0]))
            for tab in tree["children"]:
                with self.subTest(view=name, tab=tab["tab_name"]):
                    items = list(nodes(tab))
                    getters = {n.get("text_func") for n in items}
                    self.assertTrue({"GetTrxCallValue", "GetTrxCallTokenValue",
                                     "GetTrxTokenId", "GetTrxContract", "GetTrxContractWarning"} <= getters)
                    self.assertTrue({"GetTrxValue", "GetTrxValueRaw"} & getters)
                    self.assertEqual(tab["children"][0]["exist_func"], "GetTrxContractWarningExist")
                    if tab["tab_name"] == "Details":
                        data = next(n for n in items if n.get("text_func") == "GetTrxCallData")
                        self.assertEqual(data["type"], "paged_text")
                        self.assertEqual(data["text_len_func"], "GetTrxCallDataLen")

    def test_simulator_loads_shared_tron_templates_without_asset_files(self):
        firmware = (ROOT / "src/ui/gui_analyze/multi/web3/gui_general_analyze.h").read_text()
        simulator = (ROOT / "ui_simulator/simulator_model.h").read_text()
        for view in ("REMAPVIEW_TRX", "REMAPVIEW_TRX_SWAP", "REMAPVIEW_TRX_PERSONAL_MESSAGE"):
            pattern = view + r",\s*\\\n\s*(\w+)"
            self.assertEqual(re.search(pattern, firmware)[1], re.search(pattern, simulator)[1])

        source = (ROOT / "src/ui/gui_analyze/gui_analyze.c").read_text()
        loader = source[source.index("static cJSON *ParseTransactionTemplate"):
                        source.index("static void* CreateTransactionDetailWidgets")]
        harness = r'''
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "cJSON.h"
#include "gui_trx_template.h"
#define JSON_MAX_LEN (1024 * 100)
typedef int lv_fs_file_t;
typedef int lv_fs_res_t;
#define LV_FS_RES_OK 0
#define LV_FS_MODE_RD 0
static int opens, closes, mode;
static lv_fs_res_t lv_fs_open(lv_fs_file_t *fd, const char *path, int flags) {
    opens++;
    return mode == 1 ? 1 : LV_FS_RES_OK;
}
static lv_fs_res_t lv_fs_read(lv_fs_file_t *fd, void *buf, uint32_t len, uint32_t *size) {
    const char *json = "{\"type\":\"container\"}";
    memcpy(buf, json, strlen(json));
    *size = mode == 3 ? len : strlen(json);
    return mode == 2 ? 1 : LV_FS_RES_OK;
}
static void lv_fs_close(lv_fs_file_t *fd) { closes++; }
''' + loader + r'''
int main(void) {
    const char *templates[] = {GUI_TRX_PAGE_TEMPLATE, GUI_TRX_SWAP_TEMPLATE,
                              GUI_TRX_PERSONAL_MESSAGE_TEMPLATE};
    for (int i = 0; i < 3; i++) {
        cJSON *json = ParseTransactionTemplate(templates[i]);
        assert(cJSON_IsObject(json));
        assert(cJSON_HasObjectItem(json, i == 2 ? "table" : "children"));
        cJSON_Delete(json);
    }
    assert(opens == 0 && closes == 0);
#ifdef COMPILE_SIMULATOR
    cJSON *json = ParseTransactionTemplate("C:/assets/page_other.json");
    assert(cJSON_IsObject(json));
    cJSON_Delete(json);
    assert(opens == 1 && closes == 1);
    for (mode = 1; mode <= 3; mode++) {
        assert(ParseTransactionTemplate("C:/assets/page_other.json") == NULL);
    }
    assert(opens == 4 && closes == 3);
#endif
    return 0;
}
'''
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)
            (path / "check.c").write_text(harness)
            # Use the host allocator for cJSON in both loader configurations.
            subprocess.run(["cc", "-std=c11", "-DCOMPILE_SIMULATOR", "-c",
                            str(ROOT / "external/cjson/cJSON.c"), "-o", str(path / "cJSON.o")],
                           check=True)
            for defines in ([], ["-DCOMPILE_SIMULATOR"]):
                subprocess.run(["cc", "-std=c11", *defines,
                                "-I" + str(ROOT / "external/cjson"),
                                "-I" + str(ROOT / "src/ui/gui_analyze/multi/web3"),
                                str(path / "check.c"), str(path / "cJSON.o"),
                                "-o", str(path / "check")], check=True)
                subprocess.run([str(path / "check")], check=True)

    def test_c_getters_preserve_attached_values_and_long_call_data(self):
        source = (ROOT / "src/ui/gui_chain/multi/web3/gui_trx.c").read_text()
        start = source.index("#define DEFINE_TRX_DETAIL_GETTER")
        end = source.index("\nvoid TrxCheckVault", start)
        getters = source[start:end]
        fields = re.findall(r"DEFINE_TRX_DETAIL_GETTER\(\w+, (\w+)\)", getters)
        harness = r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
typedef struct { FIELDS } DisplayTronDetail;
typedef struct { DisplayTronDetail *detail; } DisplayTron;
static void strcpy_s(char *dest, size_t size, const char *source) {
    assert(strlen(source) < size);
    memcpy(dest, source, strlen(source) + 1);
}
'''.replace("FIELDS", "".join("const char *" + f + ";" for f in fields)) + getters + r'''
int main(void) {
    char data[8195];
    memset(data, 'a', sizeof(data) - 1);
    data[0] = '0'; data[1] = 'x'; data[sizeof(data) - 1] = 0;
    DisplayTronDetail detail = {
        .call_value = "1000000 TRX", .call_token_value = "9223372036854775807",
        .token_id = "1000001", .call_data = data, .contract_warning = "Unknown call"
    };
    DisplayTron tx = {&detail};
    char text[64];
    GetTrxCallValue(text, &tx, sizeof(text));
    assert(strcmp(text, detail.call_value) == 0);
    GetTrxCallTokenValue(text, &tx, sizeof(text));
    assert(strcmp(text, detail.call_token_value) == 0);
    GetTrxTokenId(text, &tx, sizeof(text));
    assert(strcmp(text, detail.token_id) == 0);
    assert(GetTrxAttachedTokenExist(NULL, &tx));
    assert(GetTrxContractWarningExist(NULL, &tx));
    int size = GetTrxCallDataLen(&tx);
    assert(size == sizeof(data));
    char *out = malloc(size);
    GetTrxCallData(out, &tx, size);
    assert(strcmp(out, data) == 0);
    free(out);
    detail.token_id = ""; detail.contract_warning = "";
    assert(!GetTrxAttachedTokenExist(NULL, &tx));
    assert(!GetTrxContractWarningExist(NULL, &tx));
    return 0;
}
'''
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)
            (path / "check.c").write_text(harness)
            subprocess.run(["cc", "-std=c11", str(path / "check.c"), "-o", str(path / "check")], check=True)
            subprocess.run([str(path / "check")], check=True)


if __name__ == "__main__":
    unittest.main()
