import re

with open('desktop-agent/src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# We need to replace the body of run_agent_loop
# It starts at fn run_agent_loop
# and ends right before fn main()

match = re.search(r'(fn run_agent_loop.*?\n\{)(.*?)(^\s*\}\s*^fn main\(\))', content, re.DOTALL | re.MULTILINE)
if match:
    print("Found run_agent_loop")
else:
    print("Could not find run_agent_loop")
