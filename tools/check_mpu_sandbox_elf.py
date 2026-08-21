import argparse
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Section:
    address: int
    size: int

    @property
    def end(self) -> int:
        return self.address + self.size

    def contains(self, address: int) -> bool:
        return self.address <= address < self.end


REQUIRED_SECTIONS = {
    ".freertos_system_calls": 0x800,
    ".sandbox_text": 0x4000,
    ".mpu_sandbox_rw": 0x4000,
    ".mpu_sandbox_stack": 0x1000,
    ".mpu_sandbox_input": 0x100,
}
ALLOWED_SANDBOX_SYSTEM_CALLS = {"MPU_xTaskGenericNotifyWait"}
FORBIDDEN_SYSCALL_TEST = "MPU_xTaskGetTickCount"


def run_objdump(objdump: str, *arguments: str) -> str:
    return subprocess.run(
        [objdump, *arguments],
        check=True,
        capture_output=True,
        text=True,
    ).stdout


def read_sections(objdump: str, elf: Path) -> dict[str, Section]:
    output = run_objdump(objdump, "-h", str(elf))
    sections: dict[str, Section] = {}
    pattern = re.compile(
        r"^\s*\d+\s+(?P<name>\.\S+)\s+(?P<size>[0-9a-fA-F]+)\s+"
        r"(?P<vma>[0-9a-fA-F]+)\s+",
        re.MULTILINE,
    )
    for match in pattern.finditer(output):
        sections[match.group("name")] = Section(
            address=int(match.group("vma"), 16),
            size=int(match.group("size"), 16),
        )
    return sections


def verify_sections(sections: dict[str, Section]) -> None:
    for name, expected_size in REQUIRED_SECTIONS.items():
        section = sections.get(name)
        if section is None:
            raise RuntimeError(f"missing required ELF section: {name}")
        if section.size != expected_size:
            raise RuntimeError(
                f"{name} size is 0x{section.size:x}, expected 0x{expected_size:x}"
            )
        if section.address % expected_size != 0:
            raise RuntimeError(
                f"{name} address 0x{section.address:x} is not 0x{expected_size:x}-aligned"
            )


def verify_calls(objdump: str, elf: Path, sections: dict[str, Section]) -> None:
    output = run_objdump(objdump, "-d", "-j", ".sandbox_text", str(elf))
    direct_call = re.compile(
        r"^\s*(?P<source>[0-9a-fA-F]+):.*\bblx?\s+"
        r"(?P<target>[0-9a-fA-F]+)\s+<(?P<symbol>[^>]+)>",
        re.MULTILINE,
    )
    allowed = (sections[".sandbox_text"], sections[".freertos_system_calls"])
    violations = []
    forbidden_test_calls = 0
    for match in direct_call.finditer(output):
        target = int(match.group("target"), 16)
        if not any(section.contains(target) for section in allowed):
            violations.append(
                f"0x{int(match.group('source'), 16):x} calls "
                f"0x{target:x} <{match.group('symbol')}>"
            )
        elif sections[".freertos_system_calls"].contains(target):
            symbol = match.group("symbol").split("+", 1)[0]
            if symbol == FORBIDDEN_SYSCALL_TEST:
                forbidden_test_calls += 1
            elif symbol not in ALLOWED_SANDBOX_SYSTEM_CALLS:
                violations.append(
                    f"0x{int(match.group('source'), 16):x} calls disallowed system call "
                    f"0x{target:x} <{match.group('symbol')}>"
                )
    if forbidden_test_calls != 1:
        violations.append(
            "sandbox must contain exactly one deliberate forbidden-syscall probe; "
            f"found {forbidden_test_calls}"
        )
    if violations:
        raise RuntimeError(
            "sandbox code calls outside its code or syscall regions:\n  "
            + "\n  ".join(violations)
        )

    indirect_calls = re.findall(
        r"^\s*[0-9a-fA-F]+:.*\bblx?\s+(?:r\d+|ip|lr)\s*$",
        output,
        re.MULTILINE,
    )
    if len(indirect_calls) != 1:
        raise RuntimeError(
            "sandbox must contain exactly one indirect call for the deliberate XN fault test; "
            f"found {len(indirect_calls)}"
        )


def verify_no_writable_globals(objdump: str, elf: Path) -> None:
    output = run_objdump(objdump, "-t", str(elf))
    symbols = {}
    for name in ("__sandbox_writable_start", "__sandbox_writable_end"):
        match = re.search(
            rf"^(?P<address>[0-9a-fA-F]+)\s+\S+.*\s{name}$",
            output,
            re.MULTILINE,
        )
        if match is None:
            raise RuntimeError(f"missing linker guard symbol: {name}")
        symbols[name] = int(match.group("address"), 16)

    if symbols["__sandbox_writable_start"] != symbols["__sandbox_writable_end"]:
        raise RuntimeError("sandbox objects define writable globals outside the sandbox RW region")


def main() -> None:
    parser = argparse.ArgumentParser(description="Verify Parser Sandbox ELF isolation")
    parser.add_argument("elf", type=Path)
    parser.add_argument("--objdump", default="arm-none-eabi-objdump")
    args = parser.parse_args()

    sections = read_sections(args.objdump, args.elf)
    verify_sections(sections)
    verify_calls(args.objdump, args.elf, sections)
    verify_no_writable_globals(args.objdump, args.elf)
    print("MPU sandbox ELF layout and call closure: PASS")


if __name__ == "__main__":
    main()
