<?php
require_once __DIR__ . '/db.php';
require_once __DIR__ . '/auth.php';

function admin_render_page(string $title, string $pageKey, string $contentHtml, string $actionsHtml = ''): string
{
    $user = admin_logged_in_user();
    $name = (string) ($user['name'] ?? ($user['username'] ?? 'Admin'));
    $email = (string) ($user['email'] ?? '');
    $initials = strtoupper(mb_substr(trim($name), 0, 1) ?: 'A');

    // Navigation with SVG icons inline
    $navGroups = [
        'Overview' => [
            ['key' => 'dashboard',     'label' => 'Dashboard',       'url' => 'index.php',         'icon' => '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/></svg>'],
        ],
        'Management' => [
            ['key' => 'devices',       'label' => 'Remote Devices',  'url' => 'devices.php',       'icon' => '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="3" width="20" height="14" rx="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>'],
            ['key' => 'users',         'label' => 'Users',           'url' => 'users.php',         'icon' => '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></svg>'],
            ['key' => 'sessions',      'label' => 'Sessions',        'url' => 'sessions.php',      'icon' => '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>'],
            ['key' => 'connections',   'label' => 'Connections',     'url' => 'connections.php',   'icon' => '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>'],
        ],
        'Billing' => [
            ['key' => 'subscriptions', 'label' => 'Subscriptions',   'url' => 'subscriptions.php', 'icon' => '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 12V22H4V12"/><path d="M22 7H2v5h20V7z"/><path d="M12 22V7"/><path d="M12 7H7.5a2.5 2.5 0 0 1 0-5C11 2 12 7 12 7z"/><path d="M12 7h4.5a2.5 2.5 0 0 0 0-5C13 2 12 7 12 7z"/></svg>'],
            ['key' => 'plans',         'label' => 'Plans',           'url' => 'plans.php',         'icon' => '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/></svg>'],
            ['key' => 'payments',      'label' => 'Payments',        'url' => 'payments.php',      'icon' => '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="1" y="4" width="22" height="16" rx="2" ry="2"/><line x1="1" y1="10" x2="23" y2="10"/></svg>'],
        ],
        'System' => [
            ['key' => 'notifications', 'label' => 'Notifications',   'url' => 'notifications.php', 'icon' => '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.73 21a2 2 0 0 1-3.46 0"/></svg>'],
            ['key' => 'reports',       'label' => 'Reports',         'url' => 'reports.php',       'icon' => '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="20" x2="18" y2="10"/><line x1="12" y1="20" x2="12" y2="4"/><line x1="6" y1="20" x2="6" y2="14"/></svg>'],
            ['key' => 'support',       'label' => 'Support',         'url' => 'support.php',       'icon' => '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>'],
            ['key' => 'audit-logs',    'label' => 'Audit Logs',      'url' => 'audit-logs.php',    'icon' => '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/><polyline points="10 9 9 9 8 9"/></svg>'],
        ],
    ];

    // Live sidebar dynamic badges
    $badges = [
        'devices' => 0,
        'sessions' => 0,
        'connections' => 0,
        'support' => 0,
    ];
    try {
        $badgePdo = admin_db();
        if (admin_table_exists($badgePdo, 'devices') && admin_column_exists($badgePdo, 'devices', 'is_online')) {
            $badges['devices'] = (int) $badgePdo->query('SELECT COUNT(*) FROM `devices` WHERE `is_online` = 1')->fetchColumn();
        }
        if (admin_table_exists($badgePdo, 'sessions') && admin_column_exists($badgePdo, 'sessions', 'status')) {
            $badges['sessions'] = (int) $badgePdo->query('SELECT COUNT(*) FROM `sessions` WHERE `status` = "active"')->fetchColumn();
        }
        if (admin_table_exists($badgePdo, 'connection_requests') && admin_column_exists($badgePdo, 'connection_requests', 'status')) {
            $badges['connections'] = (int) $badgePdo->query('SELECT COUNT(*) FROM `connection_requests` WHERE `status` = "pending"')->fetchColumn();
        }
        if (admin_table_exists($badgePdo, 'support_tickets') && admin_column_exists($badgePdo, 'support_tickets', 'status')) {
            $badges['support'] = (int) $badgePdo->query('SELECT COUNT(*) FROM `support_tickets` WHERE `status` = "open"')->fetchColumn();
        }
    } catch (Throwable $e) {
        // Fallback silently if db query fails
    }

    $navHtml = '';
    foreach ($navGroups as $groupLabel => $items) {
        $navHtml .= '<div class="nav-group"><span class="nav-group-label">' . htmlspecialchars($groupLabel, ENT_QUOTES, 'UTF-8') . '</span>';
        foreach ($items as $item) {
            $isActive = $pageKey === $item['key'];
            $badgeCount = $badges[$item['key']] ?? 0;
            $badgeHtml = '';
            if ($badgeCount > 0) {
                $badgeHtml = '<span class="nav-badge' . ($item['key'] === 'devices' ? ' nav-badge-green' : '') . '">' . $badgeCount . '</span>';
            }
            $navHtml .= '<a class="nav-link' . ($isActive ? ' active' : '') . '" href="' . htmlspecialchars(admin_url($item['url']), ENT_QUOTES, 'UTF-8') . '">'
                . '<span class="nav-icon">' . $item['icon'] . '</span>'
                . '<span class="nav-label">' . htmlspecialchars($item['label'], ENT_QUOTES, 'UTF-8') . '</span>'
                . $badgeHtml
                . ($isActive ? '<span class="nav-active-dot"></span>' : '')
                . '</a>';
        }
        $navHtml .= '</div>';
    }

    $flash = admin_get_flash();
    $flashHtml = '';
    if ($flash) {
        $flashHtml = '<div class="flash flash-' . htmlspecialchars($flash['type'], ENT_QUOTES, 'UTF-8') . '">'
            . '<span class="flash-icon">' . ($flash['type'] === 'success' ? '✓' : '⚠') . '</span>'
            . htmlspecialchars($flash['message'], ENT_QUOTES, 'UTF-8') . '</div>';
    }

    $safeTitle = htmlspecialchars($title, ENT_QUOTES, 'UTF-8');
    $safeName  = htmlspecialchars($name, ENT_QUOTES, 'UTF-8');
    $safeEmail = htmlspecialchars($email, ENT_QUOTES, 'UTF-8');
    $logoutUrl = htmlspecialchars(admin_url('logout.php'), ENT_QUOTES, 'UTF-8');
    $logoSrc   = htmlspecialchars(admin_url('assets/deskstream-logo.png'), ENT_QUOTES, 'UTF-8');
    $iconSrc   = htmlspecialchars(admin_url('assets/deskstream-icon.jpeg'), ENT_QUOTES, 'UTF-8');

    return <<<HTML
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>DeskStream Admin — {$safeTitle}</title>
    <link rel="icon" type="image/jpeg" href="{$iconSrc}">
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet">
    <style>
        /* ── Design Tokens ─────────────────────────────────────────────── */
        :root {
            --bg:             #f4f6f9;
            --surface:        #ffffff;
            --surface-raised: #ffffff;
            --border:         #e3e7ef;
            --border-subtle:  #edf0f5;

            --text-primary:   #0d1117;
            --text-secondary: #4b5563;
            --text-muted:     #8b95a6;

            --accent:         #c0392b;
            --accent-mid:     #e53935;
            --accent-soft:    #fff5f5;
            --accent-border:  rgba(192,57,43,.18);

            --success:        #16a34a;
            --success-soft:   #f0fdf4;
            --success-border: rgba(22,163,74,.18);

            --warning:        #b45309;
            --warning-soft:   #fffbeb;
            --warning-border: rgba(180,83,9,.18);

            --danger:         #dc2626;
            --danger-soft:    #fef2f2;
            --danger-border:  rgba(220,38,38,.18);

            --info:           #1d4ed8;
            --info-soft:      #eff6ff;
            --info-border:    rgba(29,78,216,.18);

            --sidebar-bg:     #0f1623;
            --sidebar-hover:  rgba(255,255,255,.05);
            --sidebar-active: rgba(192,57,43,.15);
            --sidebar-text:   #c8d4e8;
            --sidebar-muted:  #5e6e88;

            --radius-sm:  6px;
            --radius-md:  10px;
            --radius-lg:  14px;
            --radius-xl:  18px;

            --shadow-sm:  0 1px 3px rgba(0,0,0,.06), 0 1px 2px rgba(0,0,0,.04);
            --shadow-md:  0 4px 16px rgba(0,0,0,.07), 0 2px 6px rgba(0,0,0,.04);
            --shadow-lg:  0 12px 32px rgba(0,0,0,.1),  0 4px 12px rgba(0,0,0,.06);

            --sidebar-w: 240px;
            --topbar-h:  60px;

            --transition: 150ms ease;
        }

        /* ── Reset ─────────────────────────────────────────────────────── */
        *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
        html { height: 100%; -webkit-text-size-adjust: 100%; }
        body {
            height: 100%;
            font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
            font-size: 0.875rem;
            line-height: 1.5;
            color: var(--text-primary);
            background: var(--bg);
            -webkit-font-smoothing: antialiased;
        }
        a { color: inherit; text-decoration: none; }
        button { font-family: inherit; cursor: pointer; }
        img { display: block; max-width: 100%; }

        /* ── App Shell ─────────────────────────────────────────────────── */
        .app-shell {
            display: grid;
            grid-template-columns: var(--sidebar-w) minmax(0,1fr);
            min-height: 100vh;
        }

        /* ── Sidebar ───────────────────────────────────────────────────── */
        .sidebar {
            background: var(--sidebar-bg);
            display: flex;
            flex-direction: column;
            height: 100vh;
            position: sticky;
            top: 0;
            overflow-y: auto;
            overflow-x: hidden;
            z-index: 50;
            scrollbar-width: thin;
            scrollbar-color: rgba(255,255,255,.08) transparent;
        }
        .sidebar::-webkit-scrollbar { width: 4px; }
        .sidebar::-webkit-scrollbar-track { background: transparent; }
        .sidebar::-webkit-scrollbar-thumb { background: rgba(255,255,255,.08); border-radius: 4px; }

        .sidebar-brand {
            padding: 20px 16px 16px;
            border-bottom: 1px solid rgba(255,255,255,.06);
            flex-shrink: 0;
        }
        .sidebar-logo {
            display: flex;
            align-items: center;
            gap: 10px;
        }
        .sidebar-logo img {
            width: 28px;
            height: 28px;
            border-radius: 6px;
            object-fit: cover;
            flex-shrink: 0;
        }
        .sidebar-logo-text {
            font-size: 0.95rem;
            font-weight: 700;
            color: #fff;
            letter-spacing: -0.01em;
        }
        .sidebar-logo-text span { color: var(--accent-mid); }
        .sidebar-badge {
            margin-top: 4px;
            font-size: 0.62rem;
            font-weight: 600;
            letter-spacing: 0.12em;
            text-transform: uppercase;
            color: var(--sidebar-muted);
        }

        .sidebar-nav {
            flex: 1;
            padding: 12px 10px;
            display: flex;
            flex-direction: column;
            gap: 2px;
        }
        .nav-group {
            margin-bottom: 4px;
        }
        .nav-group-label {
            display: block;
            padding: 10px 8px 4px;
            font-size: 0.62rem;
            font-weight: 600;
            letter-spacing: 0.1em;
            text-transform: uppercase;
            color: var(--sidebar-muted);
        }
        .nav-link {
            display: flex;
            align-items: center;
            gap: 9px;
            padding: 8px 10px;
            border-radius: var(--radius-sm);
            color: var(--sidebar-text);
            font-size: 0.8125rem;
            font-weight: 500;
            position: relative;
            transition: background var(--transition), color var(--transition);
        }
        .nav-link:hover {
            background: var(--sidebar-hover);
            color: #fff;
        }
        .nav-link.active {
            background: var(--sidebar-active);
            color: #fff;
            font-weight: 600;
        }
        .nav-link.active .nav-icon { color: var(--accent-mid); }
        .nav-icon {
            display: flex;
            align-items: center;
            color: var(--sidebar-muted);
            flex-shrink: 0;
            transition: color var(--transition);
        }
        .nav-link:hover .nav-icon { color: #9ab; }
        .nav-label { flex: 1; }
        .nav-active-dot {
            width: 5px;
            height: 5px;
            border-radius: 50%;
            background: var(--accent-mid);
            flex-shrink: 0;
        }

        .sidebar-footer {
            padding: 12px 10px;
            border-top: 1px solid rgba(255,255,255,.06);
            flex-shrink: 0;
        }
        .sidebar-user {
            display: flex;
            align-items: center;
            gap: 10px;
            padding: 8px 10px;
            border-radius: var(--radius-sm);
            color: var(--sidebar-text);
        }
        .sidebar-avatar {
            width: 30px;
            height: 30px;
            border-radius: 50%;
            background: linear-gradient(135deg, var(--accent), var(--accent-mid));
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 0.7rem;
            font-weight: 700;
            color: #fff;
            flex-shrink: 0;
        }
        .sidebar-user-info { flex: 1; min-width: 0; }
        .sidebar-user-name {
            font-size: 0.8rem;
            font-weight: 600;
            color: #fff;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }
        .sidebar-user-email {
            font-size: 0.67rem;
            color: var(--sidebar-muted);
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }
        .sidebar-logout {
            display: flex;
            align-items: center;
            gap: 8px;
            padding: 7px 10px;
            border-radius: var(--radius-sm);
            color: var(--sidebar-muted);
            font-size: 0.78rem;
            font-weight: 500;
            margin-top: 2px;
            transition: background var(--transition), color var(--transition);
        }
        .sidebar-logout:hover {
            background: rgba(220,38,38,.12);
            color: #fca5a5;
        }
        .sidebar-logout svg { flex-shrink: 0; }

        /* ── Main Area ─────────────────────────────────────────────────── */
        .main-area {
            display: flex;
            flex-direction: column;
            min-width: 0;
            min-height: 100vh;
        }

        /* ── Topbar ─────────────────────────────────────────────────────  */
        .topbar {
            position: sticky;
            top: 0;
            z-index: 40;
            height: var(--topbar-h);
            background: rgba(244,246,249,.85);
            backdrop-filter: blur(10px);
            -webkit-backdrop-filter: blur(10px);
            border-bottom: 1px solid var(--border);
            display: flex;
            align-items: center;
            padding: 0 28px;
            gap: 16px;
        }
        .topbar-title {
            flex: 1;
            min-width: 0;
        }
        .topbar-title h1 {
            font-size: 1.0625rem;
            font-weight: 700;
            color: var(--text-primary);
            line-height: 1.2;
            letter-spacing: -0.01em;
        }
        .topbar-title .breadcrumb {
            font-size: 0.72rem;
            color: var(--text-muted);
            margin-top: 1px;
        }
        .topbar-right {
            display: flex;
            align-items: center;
            gap: 8px;
            flex-shrink: 0;
        }
        .topbar-actions { display: flex; align-items: center; gap: 8px; }

        /* ── Content Area ──────────────────────────────────────────────── */
        .content-area {
            flex: 1;
            padding: 24px 28px 48px;
            max-width: 1400px;
            width: 100%;
        }

        /* ── Flash Messages ─────────────────────────────────────────────  */
        .flash {
            display: flex;
            align-items: center;
            gap: 10px;
            padding: 10px 14px;
            border-radius: var(--radius-md);
            font-size: 0.83rem;
            font-weight: 500;
            margin-bottom: 16px;
            border: 1px solid;
        }
        .flash-success { background: var(--success-soft); border-color: var(--success-border); color: var(--success); }
        .flash-error   { background: var(--danger-soft);  border-color: var(--danger-border);  color: var(--danger); }
        .flash-icon { font-style: normal; font-size: 0.8rem; }
        /* Legacy class support */
        .flash.success { background: var(--success-soft); border-color: var(--success-border); color: var(--success); }
        .flash.error   { background: var(--danger-soft);  border-color: var(--danger-border);  color: var(--danger); }

        /* ── Stat Cards ─────────────────────────────────────────────────  */
        .card-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
            gap: 14px;
            margin-bottom: 24px;
        }
        .card {
            background: var(--surface);
            border: 1px solid var(--border);
            border-radius: var(--radius-lg);
            padding: 18px 20px;
            box-shadow: var(--shadow-sm);
            position: relative;
            overflow: hidden;
            transition: box-shadow var(--transition), transform var(--transition);
        }
        .card:hover {
            box-shadow: var(--shadow-md);
            transform: translateY(-1px);
        }
        .card .label {
            display: block;
            font-size: 0.67rem;
            font-weight: 700;
            letter-spacing: 0.08em;
            text-transform: uppercase;
            color: var(--text-muted);
            margin-bottom: 10px;
        }
        .card .value {
            font-size: 1.9rem;
            font-weight: 700;
            color: var(--text-primary);
            letter-spacing: -0.02em;
            line-height: 1;
        }
        .card .trend {
            margin-top: 8px;
            font-size: 0.72rem;
            color: var(--text-muted);
        }

        /* ── Section Panels ─────────────────────────────────────────────  */
        .section {
            background: var(--surface);
            border: 1px solid var(--border);
            border-radius: var(--radius-lg);
            box-shadow: var(--shadow-sm);
            margin-bottom: 20px;
            overflow: hidden;
        }
        .section-header {
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 12px;
            padding: 14px 20px;
            border-bottom: 1px solid var(--border-subtle);
            flex-wrap: wrap;
        }
        .section-header h3 {
            font-size: 0.875rem;
            font-weight: 600;
            color: var(--text-primary);
            letter-spacing: -0.01em;
        }
        .section-header h2 {
            font-size: 1.1rem;
            font-weight: 700;
            color: var(--text-primary);
            letter-spacing: -0.02em;
            margin-bottom: 2px;
        }

        /* ── Tables ─────────────────────────────────────────────────────  */
        .table-wrap { overflow-x: auto; }
        table {
            width: 100%;
            border-collapse: collapse;
            min-width: 600px;
        }
        th {
            padding: 10px 16px;
            text-align: left;
            font-size: 0.67rem;
            font-weight: 700;
            letter-spacing: 0.07em;
            text-transform: uppercase;
            color: var(--text-muted);
            background: var(--bg);
            border-bottom: 1px solid var(--border);
            white-space: nowrap;
        }
        td {
            padding: 12px 16px;
            font-size: 0.82rem;
            color: var(--text-secondary);
            border-bottom: 1px solid var(--border-subtle);
            vertical-align: middle;
        }
        tbody tr:last-child td { border-bottom: none; }
        tbody tr {
            transition: background var(--transition);
        }
        tbody tr:hover { background: var(--bg); }
        .text-mono {
            font-family: 'SF Mono', ui-monospace, 'Fira Code', monospace;
            font-size: 0.78rem;
            color: var(--text-primary);
        }

        /* ── Status Badges ──────────────────────────────────────────────  */
        .status-badge {
            display: inline-flex;
            align-items: center;
            gap: 5px;
            padding: 3px 8px;
            border-radius: 999px;
            font-size: 0.68rem;
            font-weight: 700;
            letter-spacing: 0.04em;
            text-transform: uppercase;
            border: 1px solid transparent;
            white-space: nowrap;
        }
        .status-badge::before {
            content: '';
            width: 5px;
            height: 5px;
            border-radius: 50%;
            background: currentColor;
            flex-shrink: 0;
        }
        .status-active,
        .status-online   { background: var(--success-soft); color: var(--success);  border-color: var(--success-border); }
        .status-inactive,
        .status-offline  { background: var(--bg);           color: var(--text-muted);border-color: var(--border); }
        .status-expired  { background: var(--danger-soft);  color: var(--danger);   border-color: var(--danger-border); }
        .status-pending  { background: var(--info-soft);    color: var(--info);     border-color: var(--info-border); }
        .status-expiring,
        .status-expiring-soon { background: var(--warning-soft); color: var(--warning); border-color: var(--warning-border); }
        .status-cancelled { background: var(--bg); color: var(--text-muted); border-color: var(--border); }
        .status-suspended { background: var(--danger-soft); color: var(--danger);   border-color: var(--danger-border); }
        .status-paid,
        .status-successful,
        .status-completed { background: var(--success-soft); color: var(--success); border-color: var(--success-border); }
        .status-failed    { background: var(--danger-soft);  color: var(--danger);  border-color: var(--danger-border); }
        .status-open      { background: var(--info-soft);    color: var(--info);    border-color: var(--info-border); }

        /* ── Buttons ─────────────────────────────────────────────────────  */
        .btn, .button, .outline-button, .danger-button, .ghost-button, .chip {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            gap: 6px;
            padding: 7px 14px;
            border-radius: var(--radius-md);
            font-size: 0.8rem;
            font-weight: 600;
            border: 1px solid transparent;
            cursor: pointer;
            text-decoration: none;
            transition: background var(--transition), border-color var(--transition), box-shadow var(--transition), transform var(--transition), color var(--transition);
            white-space: nowrap;
            line-height: 1.4;
        }
        .btn:focus-visible,
        .button:focus-visible,
        .outline-button:focus-visible,
        .danger-button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }

        /* Primary */
        .btn-primary, .button {
            background: var(--accent);
            color: #fff;
            border-color: var(--accent);
            box-shadow: 0 1px 3px rgba(192,57,43,.3);
        }
        .btn-primary:hover, .button:hover {
            background: var(--accent-mid);
            border-color: var(--accent-mid);
            box-shadow: 0 4px 12px rgba(192,57,43,.25);
            transform: translateY(-1px);
        }
        .btn-primary:active, .button:active { transform: none; box-shadow: none; }

        /* Secondary / Outline */
        .btn-secondary, .outline-button {
            background: var(--surface);
            color: var(--text-secondary);
            border-color: var(--border);
            box-shadow: var(--shadow-sm);
        }
        .btn-secondary:hover, .outline-button:hover {
            background: var(--bg);
            color: var(--text-primary);
            border-color: #c8d0dc;
        }

        /* Ghost */
        .ghost-button {
            background: transparent;
            color: var(--text-secondary);
            border-color: transparent;
        }
        .ghost-button:hover { background: var(--bg); color: var(--text-primary); }

        /* Danger */
        .danger-button {
            background: var(--danger-soft);
            color: var(--danger);
            border-color: var(--danger-border);
        }
        .danger-button:hover { background: #fee2e2; }

        /* Chip */
        .chip {
            background: var(--bg);
            color: var(--text-secondary);
            border-color: var(--border);
            padding: 5px 10px;
            font-size: 0.75rem;
        }
        .chip.active {
            background: var(--accent-soft);
            color: var(--accent);
            border-color: var(--accent-border);
        }

        /* ── Filters / Search ───────────────────────────────────────────  */
        .filters {
            display: flex;
            align-items: center;
            gap: 8px;
            flex-wrap: wrap;
        }
        .filters input[type="text"],
        .filters input[type="search"],
        .filters select {
            border: 1px solid var(--border);
            border-radius: var(--radius-md);
            padding: 7px 12px;
            font: inherit;
            font-size: 0.8rem;
            color: var(--text-primary);
            background: var(--surface);
            outline: none;
            transition: border-color var(--transition), box-shadow var(--transition);
        }
        .filters input:focus,
        .filters select:focus {
            border-color: var(--accent-mid);
            box-shadow: 0 0 0 3px rgba(229,57,53,.12);
        }

        /* ── Form Fields ────────────────────────────────────────────────  */
        .field {
            display: flex;
            flex-direction: column;
            gap: 6px;
        }
        .field label {
            font-size: 0.75rem;
            font-weight: 600;
            color: var(--text-secondary);
        }
        .field input,
        .field select,
        .field textarea {
            border: 1px solid var(--border);
            border-radius: var(--radius-md);
            padding: 9px 12px;
            font: inherit;
            font-size: 0.85rem;
            color: var(--text-primary);
            background: var(--surface);
            outline: none;
            transition: border-color var(--transition), box-shadow var(--transition);
        }
        .field input:focus,
        .field select:focus,
        .field textarea:focus {
            border-color: var(--accent-mid);
            box-shadow: 0 0 0 3px rgba(229,57,53,.12);
        }
        .field textarea { resize: vertical; min-height: 90px; }
        .grid-two {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px,1fr));
            gap: 14px;
        }

        /* ── Mini Actions ───────────────────────────────────────────────  */
        .mini-actions {
            display: flex;
            align-items: center;
            gap: 6px;
            flex-wrap: wrap;
        }

        /* ── Detail Panels (Device view etc.) ───────────────────────────  */
        .detail-panels {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(260px,1fr));
            gap: 16px;
            padding: 18px 20px;
        }
        .detail-panel {
            background: var(--bg);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 16px;
        }
        .detail-panel h4 {
            font-size: 0.65rem;
            font-weight: 700;
            letter-spacing: 0.1em;
            text-transform: uppercase;
            color: var(--text-muted);
            margin-bottom: 12px;
        }
        .detail-row {
            display: flex;
            justify-content: space-between;
            align-items: baseline;
            gap: 12px;
            padding: 7px 0;
            border-bottom: 1px solid rgba(0,0,0,.04);
            font-size: 0.82rem;
        }
        .detail-row:last-child { border-bottom: none; }
        .detail-row > span { color: var(--text-muted); flex-shrink: 0; }
        .detail-row > strong { color: var(--text-primary); font-weight: 600; text-align: right; }

        /* ── Empty States ───────────────────────────────────────────────  */
        .empty-state {
            padding: 40px 20px;
            text-align: center;
            color: var(--text-muted);
        }
        .empty-state h3 { font-size: 0.95rem; font-weight: 600; color: var(--text-secondary); margin-bottom: 4px; }
        .empty-state p  { font-size: 0.8rem; }
        td.empty-state  { padding: 32px 20px; }

        /* ── Pagination ─────────────────────────────────────────────────  */
        .pagination {
            display: flex;
            justify-content: center;
            gap: 4px;
            padding: 14px 20px 18px;
            flex-wrap: wrap;
        }
        .page-btn {
            min-width: 34px;
            padding: 6px 10px;
            border-radius: var(--radius-sm);
            border: 1px solid var(--border);
            background: var(--surface);
            color: var(--text-secondary);
            font-size: 0.8rem;
            font-weight: 600;
            transition: background var(--transition), color var(--transition), border-color var(--transition);
        }
        .page-btn:hover { background: var(--bg); color: var(--text-primary); }
        .page-btn.current {
            background: var(--accent-soft);
            color: var(--accent);
            border-color: var(--accent-border);
        }

        /* ── Mobile Nav Toggle ──────────────────────────────────────────  */
        .mobile-nav-toggle {
            display: none;
            width: 36px;
            height: 36px;
            border-radius: var(--radius-sm);
            border: 1px solid var(--border);
            background: var(--surface);
            align-items: center;
            justify-content: center;
            color: var(--text-secondary);
            flex-shrink: 0;
        }

        /* ── Responsive ─────────────────────────────────────────────────  */
        @media (max-width: 1024px) {
            :root { --sidebar-w: 210px; }
        }
        @media (max-width: 768px) {
            .app-shell { grid-template-columns: 1fr; }
            .sidebar {
                position: fixed;
                left: -100%;
                width: 260px;
                top: 0;
                bottom: 0;
                height: 100%;
                transition: left 250ms ease;
                z-index: 100;
            }
            .sidebar.open { left: 0; }
            .sidebar-overlay {
                display: none;
                position: fixed;
                inset: 0;
                background: rgba(0,0,0,.4);
                z-index: 99;
            }
            .sidebar-overlay.active { display: block; }
            .mobile-nav-toggle { display: inline-flex; }
            .content-area { padding: 16px 14px 40px; }
            .topbar { padding: 0 16px; }
            .card-grid { grid-template-columns: repeat(auto-fit, minmax(140px,1fr)); gap: 10px; }
            .section-header { padding: 12px 14px; }
            th, td { padding: 10px 12px; }
            .detail-panels { padding: 12px 14px; }
        }
        @media (max-width: 480px) {
            .card .value { font-size: 1.5rem; }
            .card { padding: 14px 16px; }
        }

        /* ── Sidebar Nav Badges ────────────────────────────────────────── */
        .nav-badge {
            margin-left: auto;
            background: rgba(255, 255, 255, 0.12);
            color: #fff;
            font-size: 0.6875rem;
            font-weight: 700;
            padding: 2px 7px;
            border-radius: 999px;
            line-height: 1;
        }
        .nav-badge.nav-badge-green {
            background: rgba(22, 163, 74, 0.25);
            color: #4ade80;
            border: 1px solid rgba(74, 222, 128, 0.3);
        }

        /* ── Sort Headers ─────────────────────────────────────────────── */
        .sort-header {
            display: inline-flex;
            align-items: center;
            gap: 4px;
            color: inherit;
            text-decoration: none;
            font-weight: 600;
        }
        .sort-header:hover {
            color: var(--accent);
        }
        .sort-header.is-sorted {
            color: var(--accent);
        }
        .sort-arrow {
            font-size: 0.85em;
            color: var(--accent);
        }

        /* ── Filter Bar ───────────────────────────────────────────────── */
        .filter-bar {
            display: flex;
            align-items: center;
            gap: 10px;
            flex-wrap: wrap;
            margin-bottom: 16px;
            background: var(--surface);
            padding: 12px 16px;
            border-radius: var(--radius-md);
            border: 1px solid var(--border);
        }
        .filter-group {
            display: flex;
            align-items: center;
            gap: 8px;
        }
        .filter-input {
            border: 1px solid var(--border);
            background: var(--bg);
            border-radius: var(--radius-sm);
            padding: 8px 12px;
            font-size: 0.85rem;
            color: var(--text-primary);
            min-width: 200px;
            transition: border-color var(--transition);
        }
        .filter-input:focus {
            outline: none;
            border-color: var(--accent);
            background: var(--surface);
        }
        .filter-select {
            border: 1px solid var(--border);
            background: var(--bg);
            border-radius: var(--radius-sm);
            padding: 8px 12px;
            font-size: 0.85rem;
            color: var(--text-primary);
            cursor: pointer;
        }
        .filter-select:focus {
            outline: none;
            border-color: var(--accent);
        }
        .filter-btn {
            background: var(--accent);
            color: #fff;
            border: none;
            padding: 8px 16px;
            border-radius: var(--radius-sm);
            font-size: 0.85rem;
            font-weight: 600;
            cursor: pointer;
            transition: opacity var(--transition);
        }
        .filter-btn:hover { opacity: 0.9; }
        .filter-reset {
            color: var(--text-muted);
            font-size: 0.825rem;
            text-decoration: underline;
            padding: 6px 8px;
        }
        .filter-reset:hover { color: var(--text-primary); }

        /* ── Pagination Bar ───────────────────────────────────────────── */
        .pagination-bar {
            display: flex;
            align-items: center;
            justify-content: space-between;
            flex-wrap: wrap;
            gap: 12px;
            padding: 16px 20px;
            border-top: 1px solid var(--border);
            background: var(--surface);
            border-bottom-left-radius: var(--radius-md);
            border-bottom-right-radius: var(--radius-md);
        }
        .pagination-info {
            font-size: 0.8125rem;
            color: var(--text-secondary);
        }
        .pagination-links {
            display: flex;
            align-items: center;
            gap: 4px;
        }
        .pagination-btn {
            min-width: 32px;
            height: 32px;
            padding: 0 10px;
            display: inline-flex;
            align-items: center;
            justify-content: center;
            border-radius: var(--radius-sm);
            border: 1px solid var(--border);
            background: var(--surface);
            color: var(--text-secondary);
            font-size: 0.8125rem;
            font-weight: 600;
            text-decoration: none;
            transition: background var(--transition), color var(--transition), border-color var(--transition);
        }
        .pagination-btn:hover:not(.disabled) {
            background: var(--bg);
            color: var(--text-primary);
            border-color: var(--text-muted);
        }
        .pagination-btn.active {
            background: var(--accent);
            color: #fff;
            border-color: var(--accent);
        }
        .pagination-btn.disabled {
            opacity: 0.4;
            cursor: not-allowed;
            pointer-events: none;
        }
        .pagination-ellipsis {
            padding: 0 6px;
            color: var(--text-muted);
        }

        /* ── Charts & Visualizations ──────────────────────────────────── */
        .chart-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));
            gap: 20px;
            margin-bottom: 24px;
        }
        .chart-card {
            background: var(--surface);
            border-radius: var(--radius-lg);
            border: 1px solid var(--border);
            box-shadow: var(--shadow-sm);
            padding: 20px;
            display: flex;
            flex-direction: column;
        }
        .chart-card-header {
            display: flex;
            align-items: center;
            justify-content: space-between;
            margin-bottom: 16px;
        }
        .chart-title {
            font-size: 0.95rem;
            font-weight: 700;
            color: var(--text-primary);
        }
        .chart-subtitle {
            font-size: 0.75rem;
            color: var(--text-muted);
            margin-top: 2px;
        }
        .chart-container {
            width: 100%;
            overflow-x: auto;
        }
        .chart-svg {
            width: 100%;
            height: auto;
            display: block;
        }
        .chart-legend {
            display: flex;
            flex-wrap: wrap;
            gap: 14px;
            margin-top: 14px;
            padding-top: 12px;
            border-top: 1px solid var(--border-subtle);
            font-size: 0.78rem;
            color: var(--text-secondary);
        }
        .legend-item {
            display: flex;
            align-items: center;
            gap: 6px;
        }
        .legend-dot {
            width: 9px;
            height: 9px;
            border-radius: 50%;
            flex-shrink: 0;
        }

        /* ── Quick KPI Pill ──────────────────────────────────────────── */
        .kpi-pill {
            display: inline-flex;
            align-items: center;
            gap: 6px;
            padding: 4px 10px;
            border-radius: 999px;
            font-size: 0.75rem;
            font-weight: 600;
            background: var(--bg);
            border: 1px solid var(--border);
        }
    </style>
</head>
<body>
    <div class="sidebar-overlay" id="sidebarOverlay" onclick="closeMobileNav()"></div>
    <div class="app-shell">
        <!-- ── Sidebar ── -->
        <aside class="sidebar" id="sidebar" role="navigation" aria-label="Main navigation">
            <div class="sidebar-brand">
                <div class="sidebar-logo">
                    <img src="{$iconSrc}" alt="DeskStream logo" width="28" height="28">
                    <div>
                        <div class="sidebar-logo-text">Desk<span>Stream</span></div>
                        <div class="sidebar-badge">Admin Portal</div>
                    </div>
                </div>
            </div>

            <nav class="sidebar-nav" aria-label="Portal navigation">
                {$navHtml}
            </nav>

            <div class="sidebar-footer">
                <div class="sidebar-user" aria-label="Signed in as {$safeName}">
                    <div class="sidebar-avatar" aria-hidden="true">{$initials}</div>
                    <div class="sidebar-user-info">
                        <div class="sidebar-user-name">{$safeName}</div>
                        <div class="sidebar-user-email">{$safeEmail}</div>
                    </div>
                </div>
                <a href="{$logoutUrl}" class="sidebar-logout" aria-label="Sign out">
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/></svg>
                    Sign out
                </a>
            </div>
        </aside>

        <!-- ── Main ── -->
        <div class="main-area">
            <header class="topbar" role="banner">
                <button class="mobile-nav-toggle" id="mobileNavToggle" aria-label="Open navigation" onclick="openMobileNav()">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><line x1="3" y1="6" x2="21" y2="6"/><line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="18" x2="21" y2="18"/></svg>
                </button>
                <div class="topbar-title">
                    <h1>{$safeTitle}</h1>
                    <div class="breadcrumb">DeskStream Admin</div>
                </div>
                <div class="topbar-right">
                    {$actionsHtml}
                </div>
            </header>

            <main class="content-area" role="main">
                {$flashHtml}
                {$contentHtml}
            </main>
        </div>
    </div>

    <script>
        function openMobileNav() {
            document.getElementById('sidebar').classList.add('open');
            document.getElementById('sidebarOverlay').classList.add('active');
            document.getElementById('mobileNavToggle').setAttribute('aria-label', 'Close navigation');
        }
        function closeMobileNav() {
            document.getElementById('sidebar').classList.remove('open');
            document.getElementById('sidebarOverlay').classList.remove('active');
            document.getElementById('mobileNavToggle').setAttribute('aria-label', 'Open navigation');
        }
        document.addEventListener('keydown', function(e) {
            if (e.key === 'Escape') closeMobileNav();
        });
    </script>
</body>
</html>
HTML;
}
