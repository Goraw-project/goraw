import os
import re

roots = [r"p:\Goraw\backend", r"p:\Goraw\std"]

count_files = 0
count_replacements = 0

pattern = re.compile(r'P:\\\\Goraw\\\\(?:llvm-project\\\\)?', re.IGNORECASE)
pattern_forward = re.compile(r'P:/Goraw/(?:llvm-project/)?', re.IGNORECASE)

for root_dir in roots:
    for dirpath, _, filenames in os.walk(root_dir):
        for fname in filenames:
            if fname.endswith(".gw"):
                fpath = os.path.join(dirpath, fname)
                try:
                    with open(fpath, "r", encoding="utf-8") as f:
                        content = f.read()
                    
                    new_content, n1 = pattern.subn("", content)
                    new_content, n2 = pattern_forward.subn("", new_content)
                    
                    if n1 > 0 or n2 > 0:
                        with open(fpath, "w", encoding="utf-8") as f:
                            f.write(new_content)
                        count_files += 1
                        count_replacements += (n1 + n2)
                except Exception as e:
                    print(f"Error reading {fpath}: {e}")

print(f"Cleaned {count_replacements} occurrences in {count_files} .gw files.")
