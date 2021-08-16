import os

failed = []

for file in os.listdir('packages'):
    name = file.replace('.json', '')
    
    pid = os.system(rf'target\release\registry.exe {name}')
    if pid == 1:
        failed.append(pid)
    
    try:
        os.remove(file)
    except:
        pass

print(failed)
