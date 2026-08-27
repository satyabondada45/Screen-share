import json
import re

transcript_path = r'C:\Users\X1 CORBON\.gemini\antigravity-ide\brain\7f73cee4-4245-490e-aef2-f56e727b012d\.system_generated\logs\transcript_full.jsonl'
lines_map = {}

with open(transcript_path, 'r', encoding='utf-8') as f:
    for line in f:
        try:
            entry = json.loads(line)
            if entry.get('type') == 'TOOL_RESPONSE' and entry.get('content'):
                content = entry['content']
                if 'File Path: ile:///c:/xampp/htdocs/Screen%20Share/desktop-agent/src/main.rs' in content:
                    # Parse the lines
                    parts = content.split('please note that any changes targeting the original code should remove the line number, colon, and leading space.')
                    if len(parts) > 1:
                        code_part = parts[1].strip()
                        for code_line in code_part.split('\n'):
                            match = re.match(r'^(\d+): (.*)', code_line)
                            if match:
                                num = int(match.group(1))
                                text = match.group(2)
                                if text == 'The above content does NOT show the entire file contents. If you need to view any lines of the file which were not shown to complete your task, call this tool again to view those lines.':
                                    continue
                                lines_map[num] = text
        except Exception as e:
            pass

print(f"Recovered {len(lines_map)} lines.")
missing = []
for i in range(1, 1648):
    if i not in lines_map:
        missing.append(i)
print(f"Missing lines count: {len(missing)}")

