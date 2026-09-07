import argparse
import subprocess
from pathlib import Path


def read_defined_symbols(objdump: str, elf: Path) -> set[str]:
    output = subprocess.run(
        [objdump, "-t", str(elf)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout

    symbols = set()
    for line in output.splitlines():
        parts = line.split()
        if len(parts) < 6 or "*UND*" in parts:
            continue
        symbols.add(parts[-1])
    return symbols


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Verify that forbidden symbols are absent from a production ELF"
    )
    parser.add_argument("elf", type=Path)
    parser.add_argument("--objdump", default="arm-none-eabi-objdump")
    parser.add_argument("--forbid", nargs="+", required=True)
    args = parser.parse_args()

    defined_symbols = read_defined_symbols(args.objdump, args.elf)
    forbidden_symbols = sorted(set(args.forbid) & defined_symbols)
    if forbidden_symbols:
        raise RuntimeError(
            "production ELF contains forbidden symbols:\n  "
            + "\n  ".join(forbidden_symbols)
        )

    print("Production forbidden-symbol check: PASS")


if __name__ == "__main__":
    main()
