import json
import os

data = {}

for file in os.listdir('indexed'):
    with open('indexed/' + file, 'r') as f:
        data.update(json.load(f))

count = 0

for (package, value) in data.items():
    count += value

print(count)