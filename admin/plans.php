<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();
$pdo = admin_db();

$table = 'plans';
$planColumns = admin_table_columns($pdo, $table);

// Handle Status Toggle (Activate / Deactivate) without deleting subscription history
if (isset($_GET['action']) && $_GET['action'] === 'toggle' && isset($_GET['id'])) {
    $planId = (int)$_GET['id'];
    $token = $_GET['token'] ?? '';
    if (!admin_verify_csrf_token($token)) {
        admin_set_flash('error', 'Invalid security token. Please try again.');
        header("Location: " . admin_url('plans.php'));
        exit;
    }

    try {
        $stmt = $pdo->prepare("SELECT `id`, `name`, `status` FROM `plans` WHERE `id` = ? LIMIT 1");
        $stmt->execute([$planId]);
        $targetPlan = $stmt->fetch(PDO::FETCH_ASSOC);

        if (!$targetPlan) {
            admin_set_flash('error', 'Plan not found.');
        } else {
            $newStatus = ($targetPlan['status'] === 'active') ? 'inactive' : 'active';
            $update = $pdo->prepare("UPDATE `plans` SET `status` = ? WHERE `id` = ?");
            $update->execute([$newStatus, $planId]);
            $statusLabel = ucfirst($newStatus);
            admin_set_flash('success', "Plan '{$targetPlan['name']}' is now {$statusLabel}.");
        }
    } catch (Throwable $e) {
        error_log("Plan toggle error: " . $e->getMessage());
        admin_set_flash('error', 'Unable to update plan status.');
    }

    header("Location: " . admin_url('plans.php'));
    exit;
}

// Handle Form Submission (Create or Edit)
if (($_SERVER['REQUEST_METHOD'] ?? '') === 'POST' && isset($_POST['plan_submit'])) {
    if (!admin_verify_csrf_token($_POST['csrf_token'] ?? null)) {
        admin_set_flash('error', 'Invalid request token. Please refresh and try again.');
    } else {
        $editId = !empty($_POST['edit_id']) ? (int)$_POST['edit_id'] : null;
        $name = trim((string)($_POST['name'] ?? ''));
        $slug = trim((string)($_POST['slug'] ?? ''));
        if ($slug === '' && $name !== '') {
            $slug = strtolower(trim(preg_replace('/[^A-Za-z0-9-]+/', '-', $name), '-'));
        }
        $description = trim((string)($_POST['description'] ?? ''));
        $priceInput = $_POST['price'] ?? null;
        $price = is_numeric($priceInput) ? max(0, (float)$priceInput) : -1;
        $durationUnit = (string)($_POST['duration_unit'] ?? 'days');
        if (!in_array($durationUnit, ['days', 'months', 'years', 'unlimited'], true)) {
            $durationUnit = 'days';
        }

        $durationValue = filter_var($_POST['duration_value'] ?? 0, FILTER_VALIDATE_INT);
        if ($durationUnit === 'unlimited') {
            $durationValue = 0;
            $durationDays = 0;
        } else {
            $durationValue = max(1, (int)$durationValue);
            if ($durationUnit === 'months') {
                $durationDays = $durationValue * 30;
            } elseif ($durationUnit === 'years') {
                $durationDays = $durationValue * 365;
            } else {
                $durationDays = $durationValue;
            }
        }

        $concurrentSessions = max(1, (int)($_POST['concurrent_sessions'] ?? 1));
        $maxDevices = max(1, (int)($_POST['max_devices'] ?? 1));
        $status = in_array($_POST['status'] ?? 'active', ['active', 'inactive'], true) ? $_POST['status'] : 'active';
        
        $canRecord = !empty($_POST['can_record']);
        $canReverseControl = !empty($_POST['can_reverse_control']);
        $maxQuality = in_array($_POST['max_quality'] ?? 'full', ['720p', '1080p', 'full'], true) ? $_POST['max_quality'] : 'full';

        $features = json_encode([
            'recording' => $canRecord,
            'reverse_control' => $canReverseControl,
            'max_quality' => $maxQuality,
            'concurrent_sessions' => $concurrentSessions,
            'audio' => true,
            'file_transfer' => true
        ]);

        try {
            if ($name === '') {
                throw new InvalidArgumentException('Please enter a valid plan name.');
            }
            if ($price < 0) {
                throw new InvalidArgumentException('Please enter a valid non-negative price.');
            }

            if ($editId !== null) {
                // UPDATE existing plan
                $sql = "
                    UPDATE `plans` SET 
                        `name` = ?, 
                        `slug` = ?, 
                        `description` = ?, 
                        `price` = ?, 
                        `duration_days` = ?, 
                        `duration_value` = ?, 
                        `duration_unit` = ?, 
                        `max_devices` = ?, 
                        `concurrent_sessions` = ?, 
                        `status` = ?, 
                        `features` = ?
                    WHERE `id` = ?
                ";
                $stmt = $pdo->prepare($sql);
                $stmt->execute([
                    $name,
                    $slug,
                    $description,
                    $price,
                    $durationDays,
                    $durationValue,
                    $durationUnit,
                    $maxDevices,
                    $concurrentSessions,
                    $status,
                    $features,
                    $editId
                ]);
                admin_set_flash('success', "Plan '{$name}' updated successfully.");
            } else {
                // Check duplicate name or slug
                $check = $pdo->prepare("SELECT id FROM `plans` WHERE `name` = ? OR (`slug` = ? AND `slug` != '') LIMIT 1");
                $check->execute([$name, $slug]);
                if ($check->fetchColumn()) {
                    throw new InvalidArgumentException("A plan with name '{$name}' or slug '{$slug}' already exists.");
                }

                // INSERT new plan
                $sql = "
                    INSERT INTO `plans` (
                        `name`, `slug`, `description`, `price`, `duration_days`, 
                        `duration_value`, `duration_unit`, `max_devices`, `concurrent_sessions`, 
                        `status`, `features`, `created_at`
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW())
                ";
                $stmt = $pdo->prepare($sql);
                $stmt->execute([
                    $name,
                    $slug,
                    $description,
                    $price,
                    $durationDays,
                    $durationValue,
                    $durationUnit,
                    $maxDevices,
                    $concurrentSessions,
                    $status,
                    $features
                ]);
                admin_set_flash('success', "Plan '{$name}' created successfully.");
            }

            // Post-Redirect-Get to prevent duplicate submissions
            header("Location: " . admin_url('plans.php'));
            exit;

        } catch (InvalidArgumentException $e) {
            admin_set_flash('error', $e->getMessage());
        } catch (Throwable $e) {
            error_log("Plan save error: " . $e->getMessage());
            admin_set_flash('error', 'Unable to save plan. Please check input and try again.');
        }
    }
}

// Check if currently editing a plan
$editPlan = null;
if (isset($_GET['edit']) && is_numeric($_GET['edit'])) {
    $editId = (int)$_GET['edit'];
    try {
        $stmt = $pdo->prepare("SELECT * FROM `plans` WHERE `id` = ? LIMIT 1");
        $stmt->execute([$editId]);
        $editPlan = $stmt->fetch(PDO::FETCH_ASSOC);
        if ($editPlan && !empty($editPlan['features'])) {
            $editPlan['features_decoded'] = json_decode($editPlan['features'], true);
        }
    } catch (Throwable $e) {
        error_log("Plan fetch error: " . $e->getMessage());
    }
}

// Fetch all plans for display
$rows = [];
try {
    $orderBy = in_array('id', $planColumns, true) ? ' ORDER BY `id` ASC' : '';
    $stmt = $pdo->query('SELECT * FROM `plans`' . $orderBy);
    $rows = $stmt->fetchAll(PDO::FETCH_ASSOC);
} catch (Throwable $e) {
    error_log("Plans list query error: " . $e->getMessage());
}

// KPI Counters
$kpiTotalPlans = count($rows);
$kpiActivePlans = count(array_filter($rows, fn($r) => strtolower($r['status'] ?? '') === 'active'));
$kpiInactivePlans = $kpiTotalPlans - $kpiActivePlans;
$kpiTotalSubscribers = (int) ($pdo->query('SELECT COUNT(*) FROM `subscriptions` WHERE status = "active"')->fetchColumn() ?: 0);

$csrfToken = htmlspecialchars(admin_csrf_token(), ENT_QUOTES, 'UTF-8');

$rowHtml = '<tr><td colspan="8" class="empty-state">No plans found.</td></tr>';
if (!empty($rows)) {
    $rowHtml = '';
    $subCountStmt = $pdo->prepare('SELECT COUNT(*) FROM `subscriptions` WHERE `plan_id` = ? AND LOWER(`status`) = "active"');
    foreach ($rows as $row) {
        $pId = (int)$row['id'];
        $status = strtolower((string)($row['status'] ?? 'active'));
        $statusUpper = strtoupper($status);
        $features = !empty($row['features']) ? json_decode($row['features'], true) : [];

        $subCountStmt->execute([$pId]);
        $activeSubsCount = (int) $subCountStmt->fetchColumn();
        
        $durationUnit = $row['duration_unit'] ?? 'days';
        $durationValue = (int)($row['duration_value'] ?? $row['duration_days'] ?? 0);
        if ($durationUnit === 'unlimited' || $durationValue === 0) {
            $durationDisplay = 'Unlimited';
        } elseif ($durationUnit === 'months') {
            $durationDisplay = $durationValue . ' month' . ($durationValue > 1 ? 's' : '');
        } elseif ($durationUnit === 'years') {
            $durationDisplay = $durationValue . ' year' . ($durationValue > 1 ? 's' : '');
        } else {
            $durationDisplay = $durationValue . ' day' . ($durationValue > 1 ? 's' : '');
        }

        $recordBadge = !empty($features['recording']) 
            ? '<span style="color:#16a34a; font-size:0.75rem;">✓ Rec</span>' 
            : '<span style="color:#94a3b8; font-size:0.75rem;">✕ Rec</span>';

        $reverseBadge = !empty($features['reverse_control']) 
            ? '<span style="color:#16a34a; font-size:0.75rem;">✓ Rev</span>' 
            : '<span style="color:#94a3b8; font-size:0.75rem;">✕ Rev</span>';

        $quality = htmlspecialchars((string)($features['max_quality'] ?? 'full'), ENT_QUOTES, 'UTF-8');
        $qualityBadge = '<span style="font-size:0.75rem; background:#f1f5f9; padding:2px 6px; border-radius:4px;">' . strtoupper($quality) . '</span>';

        $editUrl = htmlspecialchars(admin_url('plans.php', ['edit' => $pId]), ENT_QUOTES, 'UTF-8');
        $toggleUrl = htmlspecialchars(admin_url('plans.php', ['action' => 'toggle', 'id' => $pId, 'token' => admin_csrf_token()]), ENT_QUOTES, 'UTF-8');
        $toggleLabel = ($status === 'active') ? 'Deactivate' : 'Activate';

        $rowHtml .= '<tr>';
        $rowHtml .= '<td><strong>' . htmlspecialchars((string)($row['name'] ?? 'Plan')) . '</strong><br><small style="color:var(--text-muted);">' . htmlspecialchars((string)($row['slug'] ?? '')) . '</small></td>';
        $rowHtml .= '<td>₹' . number_format((float)($row['price'] ?? 0), 2) . '</td>';
        $rowHtml .= '<td>' . htmlspecialchars($durationDisplay) . '</td>';
        $rowHtml .= '<td>' . (int)($row['concurrent_sessions'] ?? 1) . ' session(s)</td>';
        $rowHtml .= '<td><span class="kpi-pill"><strong>' . $activeSubsCount . '</strong> active</span></td>';
        $rowHtml .= '<td>' . $recordBadge . ' &nbsp; ' . $reverseBadge . ' &nbsp; ' . $qualityBadge . '</td>';
        $rowHtml .= '<td><span class="status-badge status-' . $status . '">' . htmlspecialchars($statusUpper) . '</span></td>';
        $rowHtml .= '<td><div class="mini-actions">';
        $rowHtml .= '<a class="outline-button" href="' . $editUrl . '">Edit</a>';
        $rowHtml .= '<a class="outline-button" href="' . $toggleUrl . '" onclick="return confirm(\'Are you sure you want to change this plan status?\');">' . $toggleLabel . '</a>';
        $rowHtml .= '</div></td>';
        $rowHtml .= '</tr>';
    }
}

// Pre-fill form values if editing
$formTitle = $editPlan ? 'Edit Plan: ' . htmlspecialchars($editPlan['name'], ENT_QUOTES, 'UTF-8') : 'Create Plan';
$submitLabel = $editPlan ? 'Update Plan' : 'Save Plan';
$editIdVal = $editPlan ? (int)$editPlan['id'] : '';
$nameVal = htmlspecialchars($editPlan['name'] ?? '', ENT_QUOTES, 'UTF-8');
$slugVal = htmlspecialchars($editPlan['slug'] ?? '', ENT_QUOTES, 'UTF-8');
$priceVal = htmlspecialchars((string)($editPlan['price'] ?? '0.00'), ENT_QUOTES, 'UTF-8');
$unitVal = $editPlan['duration_unit'] ?? 'days';
$durationVal = htmlspecialchars((string)($editPlan['duration_value'] ?? ($editPlan['duration_days'] ?? 30)), ENT_QUOTES, 'UTF-8');
$sessionsVal = (int)($editPlan['concurrent_sessions'] ?? 1);
$devicesVal = (int)($editPlan['max_devices'] ?? 1);
$statusVal = $editPlan['status'] ?? 'active';
$descVal = htmlspecialchars($editPlan['description'] ?? '', ENT_QUOTES, 'UTF-8');

$featuresDecoded = $editPlan['features_decoded'] ?? [];
$canRecordChecked = (!isset($editPlan) || !empty($featuresDecoded['recording'])) ? 'checked' : '';
$canReverseChecked = (!isset($editPlan) || !empty($featuresDecoded['reverse_control'])) ? 'checked' : '';
$qualityVal = $featuresDecoded['max_quality'] ?? 'full';

$unitOptions = '<option value="days"' . ($unitVal === 'days' ? ' selected' : '') . '>Days</option>'
             . '<option value="months"' . ($unitVal === 'months' ? ' selected' : '') . '>Months</option>'
             . '<option value="years"' . ($unitVal === 'years' ? ' selected' : '') . '>Years</option>'
             . '<option value="unlimited"' . ($unitVal === 'unlimited' ? ' selected' : '') . '>Unlimited (No Expiry)</option>';

$statusOptions = '<option value="active"' . ($statusVal === 'active' ? ' selected' : '') . '>Active</option>'
               . '<option value="inactive"' . ($statusVal === 'inactive' ? ' selected' : '') . '>Inactive</option>';

$qualityOptions = '<option value="full"' . ($qualityVal === 'full' ? ' selected' : '') . '>Full Quality (Unrestricted)</option>'
                . '<option value="1080p"' . ($qualityVal === '1080p' ? ' selected' : '') . '>1080p Max</option>'
                . '<option value="720p"' . ($qualityVal === '720p' ? ' selected' : '') . '>720p Max (Restricted)</option>';

$cancelBtn = $editPlan 
    ? '<a class="outline-button" href="' . htmlspecialchars(admin_url('plans.php'), ENT_QUOTES, 'UTF-8') . '" style="margin-left:8px;">Cancel</a>' 
    : '';

$body = <<<HTML
<!-- Plans KPI Summary Cards -->
<div class="card-grid">
    <div class="card">
        <span class="label">Configured Plans</span>
        <p class="value">{$kpiTotalPlans}</p>
        <div class="trend">All subscription tiers</div>
    </div>
    <div class="card">
        <span class="label">Active Plans</span>
        <p class="value" style="color:#16a34a;">{$kpiActivePlans}</p>
        <div class="trend">Available for assignment</div>
    </div>
    <div class="card">
        <span class="label">Inactive Plans</span>
        <p class="value" style="color:#6b7280;">{$kpiInactivePlans}</p>
        <div class="trend">Decommissioned tiers</div>
    </div>
    <div class="card">
        <span class="label">Active Subscribers</span>
        <p class="value" style="color:#c0392b;">{$kpiTotalSubscribers}</p>
        <div class="trend">Users currently on active plans</div>
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Subscription Plans</h3>
        <div class="mini-actions">
            <button class="button" type="button" onclick="document.getElementById('plan-form').scrollIntoView({behavior:'smooth'});">Create new plan</button>
        </div>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>Plan Name</th>
                    <th>Price</th>
                    <th>Duration</th>
                    <th>Sessions</th>
                    <th>Subscribers</th>
                    <th>Entitlements</th>
                    <th>Status</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody>
                {$rowHtml}
            </tbody>
        </table>
    </div>
</div>

<div class="section" id="plan-form">
    <div class="section-header"><h3>{$formTitle}</h3></div>
    <div class="detail-panels">
        <form method="POST" style="width:100%;">
            <input type="hidden" name="csrf_token" value="{$csrfToken}">
            <input type="hidden" name="plan_submit" value="1">
            <input type="hidden" name="edit_id" value="{$editIdVal}">
            <div class="grid-two">
                <div class="field">
                    <label>Plan Name</label>
                    <input type="text" name="name" value="{$nameVal}" required placeholder="e.g. Pro Plan">
                </div>
                <div class="field">
                    <label>Slug (Unique Identifier)</label>
                    <input type="text" name="slug" value="{$slugVal}" placeholder="e.g. pro-plan (optional)">
                </div>
                <div class="field">
                    <label>Price (₹ INR)</label>
                    <input type="number" step="0.01" min="0" name="price" value="{$priceVal}" required>
                </div>
                <div class="field">
                    <label>Duration Unit</label>
                    <select name="duration_unit" id="duration_unit" onchange="toggleDurationInput()">
                        {$unitOptions}
                    </select>
                </div>
                <div class="field" id="duration_value_container">
                    <label>Duration Value</label>
                    <input type="number" min="1" name="duration_value" id="duration_value" value="{$durationVal}">
                </div>
                <div class="field">
                    <label>Max Concurrent Sessions</label>
                    <input type="number" min="1" max="100" name="concurrent_sessions" value="{$sessionsVal}" required>
                </div>
                <div class="field">
                    <label>Max Registered Devices</label>
                    <input type="number" min="1" max="100" name="max_devices" value="{$devicesVal}" required>
                </div>
                <div class="field">
                    <label>Status</label>
                    <select name="status">
                        {$statusOptions}
                    </select>
                </div>
                <div class="field">
                    <label>Streaming Quality Limit</label>
                    <select name="max_quality">
                        {$qualityOptions}
                    </select>
                </div>
                <div class="field" style="display:flex; align-items:center; gap:20px; padding-top:24px;">
                    <label style="display:flex; align-items:center; gap:8px; cursor:pointer;">
                        <input type="checkbox" name="can_record" value="1" {$canRecordChecked}>
                        <span>Allow Recording</span>
                    </label>
                    <label style="display:flex; align-items:center; gap:8px; cursor:pointer;">
                        <input type="checkbox" name="can_reverse_control" value="1" {$canReverseChecked}>
                        <span>Allow Reverse Control</span>
                    </label>
                </div>
                <div class="field" style="grid-column:1 / -1;">
                    <label>Description</label>
                    <textarea name="description" rows="3" placeholder="Plan description...">{$descVal}</textarea>
                </div>
            </div>
            <div class="mini-actions" style="padding-top:16px;">
                <button class="button" type="submit">{$submitLabel}</button>
                {$cancelBtn}
            </div>
        </form>
    </div>
</div>

<script>
function toggleDurationInput() {
    const unit = document.getElementById('duration_unit').value;
    const container = document.getElementById('duration_value_container');
    const input = document.getElementById('duration_value');
    if (unit === 'unlimited') {
        container.style.display = 'none';
        input.removeAttribute('required');
    } else {
        container.style.display = 'block';
        input.setAttribute('required', 'required');
    }
}
document.addEventListener('DOMContentLoaded', toggleDurationInput);
</script>
HTML;

echo admin_render_page('Subscription Plans', 'plans', $body);
