import re

with open('src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

old = '''let backend_url = env::var("SCREENSHARE_BACKEND_URL").unwrap_or(default_backend);'''
new = '''let backend_url = env::var("SCREENSHARE_BACKEND_URL").unwrap_or_else(|_| env::var("BACKEND_URL").unwrap_or(default_backend));'''
content = content.replace(old, new)

with open('src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)
print("Patched backend url")
