import json
import os

massive_dict = {}

for f in os.listdir('index'):
    if f.endswith('.json'):
        data = json.load(open('index/' + f))
        massive_dict.update(data)

ranked = dict(sorted(massive_dict.items(), key = lambda item : item[1], reverse = True))

index = 0

with open('indexed/ranked-900000-1000000.json', 'w') as outfile:
    json.dump(ranked, outfile)
