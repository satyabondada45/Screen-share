<?php
require_once __DIR__ . '/includes/db.php';

if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

if (!empty($_SESSION['admin_logged_in']) && !empty($_SESSION['admin_user_id'])) {
    header('Location: ' . admin_url('index.php'));
    exit();
}

$flash = admin_get_flash();
$error = '';
if ($_SERVER['REQUEST_METHOD'] === 'POST') {
    if (!isset($_POST['csrf_token']) || !hash_equals($_SESSION['admin_login_csrf'] ?? '', (string) $_POST['csrf_token'])) {
        $error = 'Invalid login request.';
    } else {
        $email = trim((string) ($_POST['email'] ?? ''));
        $password = (string) ($_POST['password'] ?? '');
        if ($email === '' || $password === '') {
            $error = 'Email and password are required.';
        } else {
            try {
                $pdo = admin_db();
                $user = admin_login_attempt($pdo, $email, $password);
                if ($user) {
                    session_regenerate_id(true);
                    $_SESSION['admin_user_id'] = (int) ($user['id'] ?? 0);
                    $_SESSION['admin_user_name'] = $user['name'] ?? ($user['username'] ?? 'Admin');
                    $_SESSION['admin_user_email'] = $user['email'] ?? $email;
                    $_SESSION['admin_logged_in'] = true;
                    header('Location: ' . admin_url('index.php'));
                    exit();
                }
                $error = 'Invalid email or password.';
            } catch (Throwable $e) {
                error_log('Admin login failure: ' . $e->getMessage());
                $error = 'Unable to sign in right now. Please try again later.';
            }
        }
    }
}

$_SESSION['admin_login_csrf'] = bin2hex(random_bytes(32));
$csrfToken = $_SESSION['admin_login_csrf'];
?>
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>DeskStream Admin — Sign In</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet">
    <style>
        :root {
            --accent:      #c0392b;
            --accent-mid:  #e53935;
            --accent-glow: rgba(192,57,43,.18);
            --surface:     #ffffff;
            --border:      #e3e7ef;
            --text:        #0d1117;
            --muted:       #6b7280;
            --danger:      #dc2626;
            --danger-soft: #fef2f2;
            --danger-bd:   rgba(220,38,38,.2);
        }
        *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
        body {
            min-height: 100vh;
            display: flex;
            font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
            -webkit-font-smoothing: antialiased;
        }
        /* Left brand panel */
        .login-brand {
            display: none;
            flex: 1;
            background: #0f1623;
            position: relative;
            overflow: hidden;
            flex-direction: column;
            padding: 48px;
        }
        .login-brand::before {
            content: '';
            position: absolute;
            inset: 0;
            background: radial-gradient(circle at 25% 15%, rgba(192,57,43,.25) 0%, transparent 50%),
                        radial-gradient(circle at 80% 85%, rgba(192,57,43,.12) 0%, transparent 45%);
        }
        .brand-logo {
            display: flex;
            align-items: center;
            gap: 11px;
            position: relative;
            z-index: 1;
        }
        .brand-logo img {
            width: 34px;
            height: 34px;
            border-radius: 7px;
            object-fit: cover;
        }
        .brand-name {
            font-size: 1.05rem;
            font-weight: 700;
            color: #fff;
            letter-spacing: -0.01em;
        }
        .brand-name em { font-style: normal; color: var(--accent-mid); }
        .brand-footer {
            position: relative;
            z-index: 1;
            margin-top: auto;
        }
        .brand-footer h2 {
            font-size: 1.7rem;
            font-weight: 700;
            color: #fff;
            line-height: 1.3;
            letter-spacing: -0.02em;
            margin-bottom: 10px;
        }
        .brand-footer p {
            font-size: 0.875rem;
            color: #7a8fa6;
            line-height: 1.6;
            max-width: 340px;
        }
        @media (min-width: 800px) { .login-brand { display: flex; } }

        /* Right form panel */
        .login-panel {
            display: flex;
            align-items: center;
            justify-content: center;
            flex-shrink: 0;
            width: 100%;
            background: var(--surface);
            padding: 40px 24px;
        }
        @media (min-width: 800px) { .login-panel { width: 440px; } }

        .form-wrap { width: 100%; max-width: 360px; }

        /* Mobile logo */
        .mobile-brand {
            display: flex;
            align-items: center;
            gap: 10px;
            margin-bottom: 36px;
        }
        .mobile-brand img { width: 30px; height: 30px; border-radius: 6px; object-fit: cover; }
        .mobile-brand-name { font-size: 1rem; font-weight: 700; color: var(--text); }
        .mobile-brand-name em { font-style: normal; color: var(--accent-mid); }
        @media (min-width: 800px) { .mobile-brand { display: none; } }

        .form-heading { margin-bottom: 26px; }
        .form-heading h1 {
            font-size: 1.5rem;
            font-weight: 700;
            color: var(--text);
            letter-spacing: -0.02em;
            line-height: 1.2;
        }
        .form-heading p { margin-top: 6px; font-size: 0.875rem; color: var(--muted); }

        .error-box {
            display: flex;
            align-items: flex-start;
            gap: 9px;
            padding: 10px 14px;
            background: var(--danger-soft);
            border: 1px solid var(--danger-bd);
            border-radius: 8px;
            font-size: 0.82rem;
            color: var(--danger);
            font-weight: 500;
            margin-bottom: 18px;
        }

        .fields { display: flex; flex-direction: column; gap: 14px; margin-bottom: 18px; }
        .field { display: flex; flex-direction: column; gap: 5px; }
        .field label { font-size: 0.77rem; font-weight: 600; color: #374151; }
        .field input {
            border: 1px solid var(--border);
            border-radius: 8px;
            padding: 10px 14px;
            font: inherit;
            font-size: 0.875rem;
            color: var(--text);
            background: #fff;
            outline: none;
            transition: border-color 150ms ease, box-shadow 150ms ease;
        }
        .field input:focus { border-color: var(--accent-mid); box-shadow: 0 0 0 3px var(--accent-glow); }
        .pw-wrap { position: relative; }
        .pw-wrap input { padding-right: 52px; width: 100%; }
        .pw-toggle {
            position: absolute;
            right: 0; top: 0; bottom: 0; width: 46px;
            display: flex; align-items: center; justify-content: center;
            border: none; background: none;
            color: var(--muted); cursor: pointer;
            font-size: 0.72rem; font-family: inherit; font-weight: 600;
            border-radius: 0 8px 8px 0;
            transition: color 150ms ease;
        }
        .pw-toggle:hover { color: var(--text); }

        .submit-btn {
            display: block; width: 100%;
            padding: 11px;
            border-radius: 8px; border: none;
            background: var(--accent);
            color: #fff;
            font: inherit; font-size: 0.9rem; font-weight: 700;
            cursor: pointer;
            box-shadow: 0 2px 8px rgba(192,57,43,.25);
            transition: background 150ms, box-shadow 150ms, transform 150ms;
            margin-bottom: 14px;
        }
        .submit-btn:hover { background: var(--accent-mid); box-shadow: 0 4px 16px rgba(192,57,43,.3); transform: translateY(-1px); }
        .submit-btn:active { transform: none; }
        .submit-btn:disabled { opacity: .6; cursor: not-allowed; transform: none; }
        .form-meta { text-align: center; font-size: 0.74rem; color: var(--muted); line-height: 1.5; }
    </style>
</head>
<body>
    <!-- Left brand panel -->
    <div class="login-brand">
        <div class="brand-logo">
            <img src="assets/deskstream-icon.jpeg" alt="DeskStream icon">
            <div class="brand-name">Desk<em>Stream</em></div>
        </div>
        <div class="brand-footer">
            <h2>Remote device management, simplified.</h2>
            <p>Monitor devices, manage subscriptions, and administer your DeskStream deployment from one place.</p>
        </div>
    </div>

    <!-- Right form panel -->
    <div class="login-panel">
        <div class="form-wrap">
            <div class="mobile-brand">
                <img src="assets/deskstream-icon.jpeg" alt="DeskStream">
                <div class="mobile-brand-name">Desk<em>Stream</em></div>
            </div>

            <div class="form-heading">
                <h1>Welcome back</h1>
                <p>Sign in to your administrator account.</p>
            </div>

            <?php if ($error !== ''): ?>
                <div class="error-box" role="alert">
                    <span aria-hidden="true">⚠</span>
                    <span><?php echo htmlspecialchars($error, ENT_QUOTES, 'UTF-8'); ?></span>
                </div>
            <?php elseif ($flash): ?>
                <div class="error-box" role="status">
                    <span aria-hidden="true">ℹ</span>
                    <span><?php echo htmlspecialchars($flash['message'], ENT_QUOTES, 'UTF-8'); ?></span>
                </div>
            <?php endif; ?>

            <form method="POST" action="<?php echo htmlspecialchars(admin_url('login.php'), ENT_QUOTES, 'UTF-8'); ?>" id="loginForm" novalidate>
                <input type="hidden" name="csrf_token" value="<?php echo htmlspecialchars($csrfToken, ENT_QUOTES, 'UTF-8'); ?>">
                <div class="fields">
                    <div class="field">
                        <label for="login-email">Email or username</label>
                        <input id="login-email" type="text" name="email" placeholder="you@example.com" required autocomplete="username" autofocus>
                    </div>
                    <div class="field">
                        <label for="login-password">Password</label>
                        <div class="pw-wrap">
                            <input id="login-password" type="password" name="password" placeholder="Enter your password" required autocomplete="current-password">
                            <button type="button" class="pw-toggle" id="pwToggle" aria-label="Show password">Show</button>
                        </div>
                    </div>
                </div>
                <button class="submit-btn" type="submit" id="submitBtn">Sign in</button>
            </form>
            <div class="form-meta">Use your DeskStream administrator credentials.</div>
        </div>
    </div>

    <script>
        var pwToggle = document.getElementById('pwToggle');
        var pwInput  = document.getElementById('login-password');
        pwToggle.addEventListener('click', function () {
            var hidden = pwInput.type === 'password';
            pwInput.type = hidden ? 'text' : 'password';
            pwToggle.textContent = hidden ? 'Hide' : 'Show';
            pwToggle.setAttribute('aria-label', hidden ? 'Hide password' : 'Show password');
        });
        document.getElementById('loginForm').addEventListener('submit', function () {
            var btn = document.getElementById('submitBtn');
            btn.disabled = true;
            btn.textContent = 'Signing in\u2026';
        });
    </script>
</body>
</html>
