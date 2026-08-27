import json
import re

transcript_path = r'C:\Users\X1 CORBON\.gemini\antigravity-ide\brain\7f73cee4-4245-490e-aef2-f56e727b012d\.system_generated\logs\transcript_full.jsonl'
lines_map = {}

with open(transcript_path, 'r', encoding='utf-8') as f:
    for line in f:
        if 'desktop-agent/src/main.rs' in line or 'desktop-agent\\\\src\\\\main.rs' in line or 'Screen%20Share/desktop-agent' in line:
            try:
                entry = json.loads(line)
                if 'content' in entry:
                    content = entry['content']
                    for part in content.split('\n'):
                        match = re.match(r'^(\d+): (.*)', part)
                        if match:
                            num = int(match.group(1))
                            text = match.group(2)
                            if 'The above content does NOT show the entire file contents' not in text:
                                lines_map[num] = text
            except Exception as e:
                pass

print(f"Recovered {len(lines_map)} lines.")

if len(lines_map) > 500:
    with open('desktop-agent/src/main.rs', 'w', encoding='utf-8') as out:
        for i in range(1, 1648):
            out.write(lines_map.get(i, '') + '\n')
    print("Wrote recovered file to desktop-agent/src/main.rs")

