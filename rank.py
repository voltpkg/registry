from pySmartDL import SmartDL
import json
import os
import requests
from progress.bar import Bar
from time import sleep
from random import randrange

import firebase_admin
from firebase_admin import credentials
from firebase_admin import firestore

cred = credentials.Certificate('config.json')
firebase_admin.initialize_app(cred)

db = firestore.client()

def add_doc(start: int, end: int, status='free'):
  db.collection('chunks').document(f'{start}-{end}').set({
    'chunk': f'{start}-{end}',
    'status': status
  })

def set_doc_status(range: str, status: str):
  db.collection('chunks').document(range).update({
    'status': status
  })

def get_doc_status(range: str) -> str:
  result = db.collection('chunks').document(range).get()
  if result.exists:
    return result.to_dict()['status']

def set_doc_complete(range: str):
  db.collection('chunks').document(range).delete()


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

while True:
    current = None
    
    for doc in db.collection('chunks').get():
        data = doc.to_dict()
        if data['status'] == 'free':
            current = data

    print('✅ Found Job')

    if current is not None:
        set_doc_complete(data['chunk'])

        START = int(data['chunk'].split('-')[0]) # 100000
        END = int(data['chunk'].split('-')[1]) # 200000
        
        pb = Bar('📈 Ranking', max=(END - START), fill='█')
        
        download_counts = {}

        index = 0
        stops = 0

        for row in rows[START:END]:
          if index <= 499:
            row = row['id']
            res = requests.get(f'http://api.npmjs.org/downloads/point/last-month/{row}')
            try:
                download_counts[row] = res.json()['downloads']
            except:
                pb.next()
                index += 1
                continue
            pb.next()
            index += 1
          else:
            download_counts = dict(sorted(download_counts.items(), key = lambda item : item[1], reverse = True))
            print('📦 Sorted Data')

            with open(f'index/ranked{(START + (500 * stops))}-{(START + (500 * stops) + 500)}.json', 'w+') as f:
                f.write(json.dumps(download_counts, indent = 4))

            stops += 1
            print('📖 Saved To ranked.json')
            download_counts = {}
            index = 0
            sleep(randrange(1000, 1400))

        pb.finish()

        
    else:
      break