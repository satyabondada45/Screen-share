import json
import re

transcript_path = r'C:\Users\X1 CORBON\.gemini\antigravity-ide\brain\7f73cee4-4245-490e-aef2-f56e727b012d\.system_generated\logs\transcript_full.jsonl'
lines_map = {}

with open(transcript_path, 'r', encoding='utf-8') as f:
    for line in f:
        if 'File Path: ile:///c:/xampp/htdocs/Screen%20Share/desktop-agent/src/main.rs' in line:
            parts = line.split('\\n')
            for part in parts:
                match = re.match(r'^(\d+): (.*)', part)
                if match:
                    num = int(match.group(1))
                    text = match.group(2)
                    if text != 'The above content does NOT show the entire file contents. If you need to view any lines of the file which were not shown to complete your task, call this tool again to view those lines.':
                        lines_map[num] = text

print(f"Recovered {len(lines_map)} lines.")

if len(lines_map) > 500:
    with open('desktop-agent/src/main.rs', 'w', encoding='utf-8') as out:
        for i in range(1, max(lines_map.keys()) + 1):
            out.write(lines_map.get(i, '') + '\n')
    print("Wrote recovered file to desktop-agent/src/main.rs")

