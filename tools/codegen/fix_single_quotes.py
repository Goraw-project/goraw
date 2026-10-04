import os

count = 0
fixed_files = []
for root, _, files in os.walk(r'p:\Goraw'):
    for f in files:
        if f.endswith('.gw'):
            path = os.path.join(root, f)
            with open(path, 'r', encoding='utf-8', errors='ignore') as fp:
                content = fp.read()
            if "'''" in content:
                new_content = content.replace("'''", r"'\''")
                with open(path, 'w', encoding='utf-8', newline='\n') as fp:
                    fp.write(new_content)
                count += 1
                fixed_files.append(path)

print(f"Total files fixed: {count}")
for f in fixed_files:
    print(f" - {f}")
