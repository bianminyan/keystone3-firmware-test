import sys
from pathlib import Path
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from export_comments import strip_new


class CommentTests(unittest.TestCase):
    def clean(self, old, new, path='a.c'):
        return strip_new(old.encode(), new.encode(), path)[0].decode()

    def test_history_license_and_strings(self):
        old = '// history\nint x;\n'
        new = '// history\n// added\nint x; // note\nconst char *s = "https://test/*literal*/";\n/* SPDX-License-Identifier: MIT */\n'
        self.assertEqual(self.clean(old,new), '// history\nint x;\nconst char *s = "https://test/*literal*/";\n/* SPDX-License-Identifier: MIT */\n')

    def test_rust_literals_and_nested_blocks(self):
        s = 'let s = r##"// literal /* here */"##;\n/* outer /* inner */ end */\nfn f(x: &\'a str) {} // note\n'
        self.assertEqual(self.clean('',s,'a.rs'), 'let s = r##"// literal /* here */"##;\nfn f(x: &\'a str) {}\n')

    def test_python_directives(self):
        s = '#!/usr/bin/python3\n# note\ns = "# literal" # note\nx = 1 # noqa\n'
        out = self.clean('',s,'a.py')
        self.assertIn('#!/usr/bin/python3',out)
        self.assertIn('s = "# literal"\n',out)
        self.assertIn('# noqa',out)
        self.assertNotIn('# note',out)

    def test_cmake_literals(self):
        self.assertEqual(self.clean('', '# note\nset(X "# literal") # note\n', 'CMakeLists.txt'), 'set(X "# literal")\n')

    def test_partial_historical_block_fails(self):
        with self.assertRaisesRegex(ValueError, 'Partly changed'):
            self.clean('/* old\n kept */\n', '/* changed\n kept */\n')

    def test_inline_separator(self):
        self.assertEqual(self.clean('', 'int/* note */value;\n'), 'int value;\n')
