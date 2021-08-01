from pySmartDL import SmartDL
import json
import os
import requests
from progress.bar import Bar


print('⌛ Downloading Dataset')
if '_all_docs.json' not in os.listdir():
    url = 'https://replicate.npmjs.com/_all_docs'

    obj = SmartDL(url, './_all_docs.json', threads = 5)
    obj.start()

data = None

with open('_all_docs.json', 'r') as f:
    data = json.load(f)

rows = data['rows']
print('⚡ Loaded Dataset')

NUMBER = 500
pb = Bar('📈 Ranking', max=NUMBER, fill='█')

download_counts = {}

for row in rows[:NUMBER]:
    row = row['id']
    res = requests.get(f'http://api.npmjs.org/downloads/point/last-month/{row}')
    try:
        download_counts[row] = res.json()['downloads']
    except:
        pb.next()
        continue
    pb.next()

pb.finish()

download_counts = dict(sorted(download_counts.items(), key = lambda item : item[1], reverse = True))
print('📦 Sorted Data')

with open('ranked.json', 'w+') as f:
    f.write(json.dumps(download_counts, indent = 4))

print('📖 Saved To ranked.json')