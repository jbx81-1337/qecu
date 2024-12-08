# Simple Helper tool to generate a memory_map Rhai file from a .h
import sys
import re

re_groups = r"#define ([A-Z0-9_]+).*(0x[A-F0-9]{8})"

if __name__ == '__main__':
    print("fn memory_map(){")
    print("    return #{")
    for line in sys.stdin:
        matches = re.findall(re_groups, line)
        matches = matches[0]
        print(f"        {matches[0]}: {matches[1]},")
    print("    };")
    print("};")
