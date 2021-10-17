import json
import os
from math import floor, ceil

data = {}

for file in os.listdir('indexed'):
    with open('indexed/' + file, 'r') as f:
        data.update(json.load(f))

download_counts = dict(sorted(data.items(), key = lambda item : item[1], reverse = True))

total_count = 0

for (package, value) in download_counts.items():
    total_count += value

top_1_pc = floor(1 / 100 * len(download_counts.items()))
top_1_count = 0

keys = list(download_counts.keys())

for key in keys[:top_1_pc]:
    top_1_count += download_counts[key]

print(f'Top 1 %: {top_1_count} vs Total: {total_count}')
print(f'Top 1 % Forms {(top_1_count / total_count) * 100}% of the {total_count} total downloads')
print(f'That\'s {ceil(0.01 * len(download_counts))} packages of {len(download_counts)} total packages')