import os

failed = []

for file in os.listdir('packages'):
    file = file.replace('.json', '')
    print(file)
    pid = os.system(rf'target\release\registry.exe {file}')
    if pid == 1:
        failed.append(pid)

print(failed)
