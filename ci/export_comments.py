"""Remove newly introduced source comments while preserving literals and legal notices."""
import ast
import difflib
import io
from pathlib import Path
import re
import tokenize

KEEP = re.compile(r'copyright|spdx|licen[sc]e|permission is hereby granted|redistribution|all rights reserved|clang-format|nolint|iwyu|fall.?through|noqa|type:\s*ignore|pylint|fmt:|coding[:=]|^#!', re.I)
LEGAL = re.compile(r'copyright|spdx|licen[sc]e|permission is hereby granted|redistribution|all rights reserved', re.I)
SUFFIXES = {'.c', '.h', '.cpp', '.hpp', '.cc', '.rs', '.py', '.cmake', '.ld'}


def supported(path):
    return Path(path).suffix in SUFFIXES or Path(path).name == 'CMakeLists.txt'


def spans(text, path):
    if Path(path).suffix == '.py':
        offsets = [0]
        for line in text.splitlines(keepends=True):
            offsets.append(offsets[-1] + len(line))
        return [(offsets[t.start[0]-1]+t.start[1], offsets[t.end[0]-1]+t.end[1])
                for t in tokenize.generate_tokens(io.StringIO(text).readline)
                if t.type == tokenize.COMMENT]
    rust = path.endswith('.rs')
    cmake = path.endswith('.cmake') or Path(path).name == 'CMakeLists.txt'
    result = []
    i = 0
    while i < len(text):
        if cmake:
            bracket = re.match(r'(#)?\[(=*)\[', text[i:])
            if bracket:
                endmark = ']' + bracket[2] + ']'
                end = text.find(endmark, i + len(bracket[0]))
                if end < 0:
                    raise ValueError('Unclosed CMake bracket: ' + path)
                end += len(endmark)
                if bracket[1]:
                    result.append((i, end))
                i = end
                continue
        else:
            raw = re.match(r'(?:br|cr|r)(#{0,255})"', text[i:]) if rust else re.match(r'(?:u8|u|U|L)?R"([^ ()\\\t\r\n]{0,16})\(', text[i:])
            if raw:
                endmark = '"' + raw[1] if rust else ')' + raw[1] + '"'
                end = text.find(endmark, i + len(raw[0]))
                if end < 0:
                    raise ValueError('Unclosed raw string: ' + path)
                i = end + len(endmark)
                continue
        if not rust and not cmake:
            number = re.match(r"(?:[0-9]|\.[0-9])(?:[eEpP][+-]|[A-Za-z0-9_.]|'[A-Za-z0-9_])*", text[i:])
            if number:
                i += len(number[0])
                continue
        if text[i] == '"' or (not cmake and text[i] == "'" and (not rust or re.match(r"'(?:[^'\\\n]|\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.))'", text[i:]))):
            quote = text[i]
            i += 1
            while i < len(text):
                if text[i] == '\\':
                    i += 2
                elif text[i] == quote:
                    i += 1
                    break
                else:
                    i += 1
            continue
        if (cmake and text[i] == '#') or (not cmake and text.startswith('//', i)):
            end = text.find('\n', i)
            if end < 0:
                end = len(text)
            if not rust and not cmake:
                while end < len(text) and text[i:end].rstrip('\r').endswith('\\'):
                    end = text.find('\n', end + 1)
                    if end < 0:
                        end = len(text)
            result.append((i, end))
            i = end
        elif not cmake and text.startswith('/*', i):
            start, depth = i, 1
            i += 2
            while depth and i < len(text):
                if rust and text.startswith('/*', i):
                    depth += 1
                    i += 2
                elif text.startswith('*/', i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
            if depth:
                raise ValueError('Unclosed block comment: ' + path)
            result.append((start, i))
        elif cmake and text[i] == '\\':
            i += 2
        else:
            i += 1
    return result


def normalize(text):
    return re.sub(r'\s+', ' ', text).strip()


def strip_new(before, after, path):
    old = before.decode('utf-8')
    new = after.decode('utf-8')
    old_spans = spans(old, path)
    new_spans = spans(new, path)
    historical = {normalize(old[a:b]) for a, b in old_spans}
    changed = set()
    for tag, _, _, a, b in difflib.SequenceMatcher(None, old.splitlines(), new.splitlines(), autojunk=False).get_opcodes():
        if tag in ('insert', 'replace'):
            changed.update(range(a + 1, b + 1))
    protected = set()
    # A legal notice may consist of multiple consecutive // or # comment lines.
    group = []
    for span in new_spans + [(len(new)+1, len(new)+1)]:
        if group and new[group[-1][1]:span[0]].strip():
            if LEGAL.search('\n'.join(new[a:b] for a,b in group)):
                protected.update(group)
            group = []
        group.append(span)
    if group and LEGAL.search('\n'.join(new[a:b] for a,b in group if a <= len(new))):
        protected.update(group)
    edits = []
    for a,b in new_spans:
        rows = set(range(new.count('\n', 0, a)+1, new.count('\n', 0, b-1)+2))
        if not rows & changed or normalize(new[a:b]) in historical or (a,b) in protected or KEEP.search(new[a:b]):
            continue
        if not rows <= changed:
            raise ValueError('Partly changed multiline comment requires manual cleanup: ' + path)
        start = new.rfind('\n', 0, a)+1
        end = new.find('\n', b)
        end = len(new) if end < 0 else end
        if not new[start:a].strip() and not new[b:end].strip():
            previous_start = new.rfind('\n', 0, max(0, start-1))+1
            previous_line = new[previous_start:max(0, start-1)]
            continued = (not path.endswith(('.rs', '.py', '.cmake'))
                         and Path(path).name != 'CMakeLists.txt'
                         and previous_line.rstrip(' \t\r').endswith('\\'))
            newline = '\r\n' if end > 0 and new[end-1:end] == '\r' else '\n'
            edits.append((start, min(end+1, len(new)), newline if continued else ''))
        else:
            # Preserve token separation and preprocessor line boundaries.
            replacement = '\n' * new[a:b].count('\n') or ' '
            if not new[b:end].strip():
                while a > start and new[a-1] in ' \t':
                    a -= 1
                b = end
                replacement = ''
            edits.append((a,b,replacement))
    result = new
    for a,b,replacement in reversed(edits):
        result = result[:a] + replacement + result[b:]
    if path.endswith('.py'):
        if ast.dump(ast.parse(new)) != ast.dump(ast.parse(result)):
            raise ValueError('Python semantics changed: ' + path)
    return result.encode('utf-8'), len(edits)


def check_unsupported(before, after, path):
    if Path(path).suffix not in {'.sh', '.bash', '.yml', '.yaml', '.toml', '.js', '.ts', '.tsx', '.jsx'}:
        return
    old, new = before.decode('utf-8').splitlines(), after.decode('utf-8').splitlines()
    for kind, _, _, a, b in difflib.SequenceMatcher(None, old, new, autojunk=False).get_opcodes():
        if kind not in ('insert', 'replace'):
            continue
        for line in new[a:b]:
            if re.search(r'#|//|/\*', line) and line not in old and not KEEP.search(line):
                raise ValueError('New possible comment in unsupported syntax; clean or review manually: ' + path)
