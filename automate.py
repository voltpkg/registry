import os

failed = []

for file in os.listdir('idek'):
    name = file.replace('.json', '')
    if len(name) != 128:
        pid = os.system(rf'target\release\registry.exe {name}')
        if pid == 1:
            failed.append(pid)
        
        try:
            os.remove(file)
        except:
            pass

print(failed)
