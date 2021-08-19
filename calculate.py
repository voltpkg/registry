import json
import os
from math import floor

data = {}

for file in os.listdir('indexed'):
    with open('indexed/' + file, 'r') as f:
        data.update(json.load(f))

total_count = 0

for (package, value) in data.items():
    total_count += value

top_1_pc = floor(1 / 100 * len(data.items()))
top_1_count = 0

keys = list(data.keys())

for key in keys[:top_1_pc]:
    top_1_count += data[key]

print(f'Top 50%: {top_1_count} vs Total: {total_count}')
print(f'Top 50% Forms {(top_1_count / total_count) * 100}% of the {total_count} total downloads')