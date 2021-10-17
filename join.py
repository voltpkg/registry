import json
import os

massive_dict = {}

for f in os.listdir('new'):
    if f.endswith('.json'):
        data = json.load(open('new/' + f))
        massive_dict.update(data)

ranked = dict(sorted(massive_dict.items(), key = lambda item : item[1], reverse = True))

index = 0

with open('indexed/ranked-700000-800000.json', 'w') as outfile:
    json.dump(ranked, outfile)
