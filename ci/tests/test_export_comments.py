import sys
import shutil
import subprocess
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

    @unittest.skipUnless(shutil.which('clang'), 'clang is required for preprocessing regression')
    def test_comment_after_macro_continuation_preserves_preprocessing(self):
        for ending in ('\n', '\r\n'):
            for comment in ('/* note */', '// note', '/* first' + ending + 'second */'):
                source = '#define X \\' + ending + comment + ending + 'int value;' + ending
                cleaned = self.clean('', source)
                self.assertNotIn('note', cleaned)
                self.assertNotIn('/*', cleaned)
                def preprocess(text):
                    p = subprocess.run(['clang', '-E', '-P', '-x', 'c', '-'], input=text, text=True, capture_output=True)
                    self.assertEqual(p.returncode, 0, p.stderr)
                    return [line.strip() for line in p.stdout.splitlines() if line.strip()]
                self.assertEqual(preprocess(source), ['int value;'])
                self.assertEqual(preprocess(cleaned), preprocess(source))

    @unittest.skipUnless(shutil.which('clang++'), 'clang++ is required for C++ regression')
    def test_cpp_digit_separators_preserve_literals_and_compile(self):
        source = "int n = 1'000;\nconst char *s = \"'//literal\"; // remove\nunsigned h = 0xA'B;\ndouble f = 1.2'34e+1;\nchar c = '\"';\n"
        cleaned = self.clean('', source, 'a.cpp')
        self.assertIn("1'000", cleaned)
        self.assertIn('"\'//literal"', cleaned)
        self.assertNotIn('// remove', cleaned)
        for text in (source, cleaned):
            p = subprocess.run(['clang++', '-std=c++14', '-fsyntax-only', '-x', 'c++', '-'], input=text, text=True, capture_output=True)
            self.assertEqual(p.returncode, 0, p.stderr)

    @unittest.skipUnless(shutil.which('clang++'), 'clang++ is required for token boundary regression')
    def test_identifier_digit_adjacent_to_character_literal(self):
        source = "#define S(x) #x\nconst char *s = S(foo1'a');\nconst char *t = \"'//literal\"; // remove\n"
        cleaned = self.clean('', source, 'a.cpp')
        self.assertIn("S(foo1'a')", cleaned)
        self.assertIn('"\'//literal"', cleaned)
        self.assertNotIn('// remove', cleaned)
        outputs = []
        for text in (source, cleaned):
            compiled = subprocess.run(['clang++', '-std=c++14', '-fsyntax-only', '-x', 'c++', '-'], input=text, text=True, capture_output=True)
            self.assertEqual(compiled.returncode, 0, compiled.stderr)
            preprocessed = subprocess.run(['clang++', '-std=c++14', '-E', '-P', '-x', 'c++', '-'], input=text, text=True, capture_output=True)
            self.assertEqual(preprocessed.returncode, 0, preprocessed.stderr)
            outputs.append(preprocessed.stdout)
        self.assertEqual(outputs[0], outputs[1])
