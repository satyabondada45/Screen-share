import os
import re

html_path = 'desktop-agent/assets/dashboard.html'
with open(html_path, 'r', encoding='utf-8') as f:
    html = f.read()

# 1. Update Topbar Tabs Container
topbar_html = '''        <div class="topbar-center" id="topbarTabsContainer" style="display:flex; gap:8px;">
            <button class="topbar-tab-btn active" type="button" id="dashboardTabBtn" onclick="switchSessionTab('dashboard')">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <rect x="2" y="3" width="20" height="14" rx="2" />
                    <line x1="8" y1="21" x2="16" y2="21" />
                    <line x1="12" y1="17" x2="12" y2="21" />
                </svg>
                Dashboard
            </button>
            <!-- Dynamic session tabs will be inserted here -->
        </div>'''
html = re.sub(r'<div class="topbar-center">.*?</div>', topbar_html, html, flags=re.DOTALL | re.IGNORECASE)

# 2. Add Tab State variables and replace openIntegratedSession/endActiveSession
session_js = '''        // --- MULTI-SESSION TABS ---
        let activeSessions = {}; // { targetId: { name: string, iframe: element, tabBtn: element } }
        let currentActiveTab = 'dashboard';

        function switchSessionTab(tabId) {
            currentActiveTab = tabId;
            const inner = document.getElementById('dashboardInnerContent');
            const mainContent = document.getElementById('mainContentArea');
            
            document.querySelectorAll('.topbar-tab-btn').forEach(btn => btn.classList.remove('active'));
            
            if (tabId === 'dashboard') {
                const dbBtn = document.getElementById('dashboardTabBtn');
                if(dbBtn) dbBtn.classList.add('active');
                inner.style.display = 'block';
                mainContent.classList.remove('a-session-active');
                
                Object.values(activeSessions).forEach(s => s.iframe.style.display = 'none');
            } else {
                const session = activeSessions[tabId];
                if (session) {
                    session.tabBtn.classList.add('active');
                    inner.style.display = 'none';
                    mainContent.classList.add('a-session-active');
                    
                    Object.values(activeSessions).forEach((s, key) => {
                        s.iframe.style.display = (key === tabId) ? 'block' : 'none';
                    });
                }
            }
        }

        function openIntegratedSession(url) {
            const targetId = new URL(url, window.location.origin).searchParams.get('id');
            const targetName = new URL(url, window.location.origin).searchParams.get('target_name') || 'Remote Device';
            
            if (activeSessions[targetId]) {
                switchSessionTab(targetId);
                return;
            }
            
            const iframe = document.createElement('iframe');
            iframe.src = url + "&integrated=1";
            iframe.id = 'sessionIframe_' + targetId;
            iframe.className = 'remote-session-iframe';
            iframe.style.display = 'none';
            iframe.style.position = 'relative';
            iframe.style.width = '100%';
            iframe.style.height = '100%';
            iframe.style.zIndex = '50';
            iframe.style.margin = '0';
            iframe.style.border = 'none';
            iframe.style.flex = '1';
            iframe.allow = "fullscreen; clipboard-read; clipboard-write";
            
            document.getElementById('mainContentArea').appendChild(iframe);
            
            const tabBtn = document.createElement('button');
            tabBtn.className = 'topbar-tab-btn';
            tabBtn.innerHTML = `
                <span style="margin-right:8px;font-size:16px;color:#10b981;">●</span>
                <span style="flex:1;text-align:left;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">${targetName}</span>
                <span class="tab-close-btn" onclick="event.stopPropagation(); closeSessionTab('${targetId}')" style="margin-left:8px;padding:2px;cursor:pointer;">✕</span>
            `;
            tabBtn.onclick = () => switchSessionTab(targetId);
            document.getElementById('topbarTabsContainer').appendChild(tabBtn);
            
            activeSessions[targetId] = { name: targetName, iframe, tabBtn };
            switchSessionTab(targetId);
        }

        function closeSessionTab(targetId) {
            const session = activeSessions[targetId];
            if (!session) return;
            
            if (session.iframe.contentWindow) {
                session.iframe.contentWindow.postMessage('end_session_command', '*');
            }
            
            setTimeout(() => {
                finishClosingSessionTab(targetId);
            }, 500);
        }

        function finishClosingSessionTab(targetId) {
            const session = activeSessions[targetId];
            if (!session) return;
            
            session.iframe.remove();
            session.tabBtn.remove();
            delete activeSessions[targetId];
            
            if (currentActiveTab === targetId) {
                switchSessionTab('dashboard');
            }
            fetchAllDevices();
        }

        // We override the old single-session endActiveSession for fallback UI cases
        function endActiveSession() {
            if (currentActiveTab !== 'dashboard') {
                closeSessionTab(currentActiveTab);
            }
        }
'''

# Replace openIntegratedSession and endActiveSession blocks
html = re.sub(r'function openIntegratedSession\(url\) \{.*?(?=let integratedSessionEntered = false;)', session_js, html, flags=re.DOTALL)

# 3. Message Listener Updates
msg_listener = '''        let integratedSessionEntered = false;
        window.addEventListener('message', function (e) {
            if (e.data === 'session_connected') {
                syncBSessionStateFromHealth();
            } else if (e.data && e.data.type === 'end_integrated_session') {
                finishClosingSessionTab(e.data.targetId);
            } else if (e.data === 'end_integrated_session') {
                // Fallback for single session if targetId isn't provided
                finishClosingSessionTab(currentActiveTab);
            } else if (e.data && e.data.type === 'reverse_remote_approved') {
                const requesterId = String(e.data.requester_system_id || '');'''

html = re.sub(r'let integratedSessionEntered = false;.*?else if \(e\.source === document\.getElementById\(\'remoteSessionIframe\'\)\?\.contentWindow\s+&& e\.data\?\.type === \'reverse_remote_approved\'\) \{.*?const requesterId = String\(e\.data\.requester_system_id \|\| \'\'\);', msg_listener, html, flags=re.DOTALL)

# Remove the old static single iframe
html = html.replace('''            <!-- Integrated Remote Session Iframe -->
            <iframe id="remoteSessionIframe"
                style="display: none; width: 100%; height: calc(100vh - 200px); border: none; z-index: 50; background: #fff; margin-top: 20px;"
                allow="fullscreen; clipboard-read; clipboard-write"></iframe>''', '')

# Remove enterConnectedSession
html = re.sub(r'<script>\s*function enterConnectedSession\(\).*?\}\s*document\.addEventListener', '<script>\ndocument.addEventListener', html, flags=re.DOTALL)

with open(html_path, 'w', encoding='utf-8') as f:
    f.write(html)
print('Patched successfully!')
