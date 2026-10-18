<?php
require_once __DIR__ . '/api_path.php';
$apiBasePath = deskstream_api_base_path();
if (session_status() === PHP_SESSION_NONE) {
    session_start();
}
if (empty($_SESSION['user_id'])) {
    header('Location: index.php');
    exit();
}

require_once deskstream_service_file('SubscriptionService.php');
$userId = (int)$_SESSION['user_id'];
$csrfToken = $_SESSION['payment_csrf_token'] ?? bin2hex(random_bytes(32));
$_SESSION['payment_csrf_token'] = $csrfToken;
$current = SubscriptionService::getUserSubscription($userId);
$entitlements = SubscriptionService::getUserEntitlements($userId);
$plans = array_values(array_filter(
    SubscriptionService::getActivePlans(),
    static fn(array $plan): bool => in_array($plan['slug'] ?? '', ['monthly', 'yearly'], true)
));
$currentPlanName = htmlspecialchars((string)($entitlements['plan_name'] ?? 'Free Plan'), ENT_QUOTES, 'UTF-8');
$currentExpiry = $current['expires_at'] ?? null;
$currentExpiryDisplay = $currentExpiry
    ? htmlspecialchars((new DateTimeImmutable($currentExpiry, new DateTimeZone('UTC')))->format('Y-m-d H:i') . ' UTC', ENT_QUOTES, 'UTF-8')
    : 'Ongoing';
$planData = array_map(static fn(array $plan): array => [
    'id' => (int)$plan['id'],
    'name' => (string)$plan['name'],
    'price' => (float)$plan['price'],
    'duration_value' => (int)$plan['duration_value'],
    'duration_unit' => (string)$plan['duration_unit']
], $plans);
?>
<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>DeskStream subscription</title>
    <style>
        body{font-family:Segoe UI,Arial,sans-serif;margin:0;padding:32px;background:#f1f5f9;color:#0f172a}
        main{max-width:900px;margin:auto}.card{background:#fff;border:1px solid #cbd5e1;border-radius:12px;padding:24px;margin:16px 0}
        .plans{display:grid;grid-template-columns:repeat(auto-fit,minmax(250px,1fr));gap:16px}
        button{border:0;border-radius:8px;background:#2563eb;color:white;padding:12px 18px;font-weight:600;cursor:pointer}
        button:disabled{opacity:.6;cursor:wait}#status{min-height:1.5em;color:#b91c1c}
    </style>
</head>
<body>
<main>
    <h1>DeskStream subscription</h1>
    <section class="card" aria-live="polite">
        <strong>Current plan:</strong> <?= $currentPlanName ?><br>
        <strong>Expires:</strong> <?= $currentExpiryDisplay ?>
    </section>
    <section class="plans" id="plans"></section>
    <p id="status" role="status" aria-live="polite"></p>
</main>
<script src="https://checkout.razorpay.com/v1/checkout.js"></script>
<script>
const PAYMENT_CSRF_TOKEN = <?= json_encode($csrfToken, JSON_HEX_TAG | JSON_HEX_AMP | JSON_HEX_APOS | JSON_HEX_QUOT) ?>;
const PAID_PLANS = <?= json_encode($planData, JSON_HEX_TAG | JSON_HEX_AMP | JSON_HEX_APOS | JSON_HEX_QUOT) ?>;
const plansContainer = document.getElementById("plans");
const statusBox = document.getElementById("status");

for (const plan of PAID_PLANS) {
    const card = document.createElement("article");
    card.className = "card";
    const title = document.createElement("h2");
    title.textContent = plan.name;
    const price = document.createElement("p");
    price.textContent = `₹${plan.price.toFixed(2)} / ${plan.duration_value} ${plan.duration_unit}`;
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = "Continue to secure payment";
    button.addEventListener("click", () => startPayment(plan, button));
    card.append(title, price, button);
    plansContainer.append(card);
}

async function startPayment(plan, button) {
    if (!window.Razorpay) {
        statusBox.textContent = "Secure payment checkout is unavailable. Reload and try again.";
        return;
    }
    button.disabled = true;
    statusBox.textContent = "";
    try {
        const orderResponse = await fetch(<?= json_encode($apiBasePath . '/payments/create.php', JSON_HEX_TAG | JSON_HEX_AMP | JSON_HEX_APOS | JSON_HEX_QUOT) ?>, {
            method: "POST",
            credentials: "same-origin",
            headers: {"Content-Type": "application/json", "X-CSRF-Token": PAYMENT_CSRF_TOKEN},
            body: JSON.stringify({plan_id: plan.id})
        });
        const order = await orderResponse.json();
        if (!orderResponse.ok || order.status !== "success") {
            throw new Error(order.message || "Could not create a payment order.");
        }
        const checkout = new Razorpay({
            key: order.key_id,
            order_id: order.order_id,
            amount: order.amount,
            currency: order.currency,
            name: "DeskStream",
            description: order.plan_name,
            handler: async payment => {
                try {
                    const verificationResponse = await fetch(<?= json_encode($apiBasePath . '/payments/verify.php', JSON_HEX_TAG | JSON_HEX_AMP | JSON_HEX_APOS | JSON_HEX_QUOT) ?>, {
                        method: "POST",
                        credentials: "same-origin",
                        headers: {"Content-Type": "application/json", "X-CSRF-Token": PAYMENT_CSRF_TOKEN},
                        body: JSON.stringify({
                            razorpay_order_id: payment.razorpay_order_id,
                            razorpay_payment_id: payment.razorpay_payment_id,
                            razorpay_signature: payment.razorpay_signature
                        })
                    });
                    const verification = await verificationResponse.json();
                    if (!verificationResponse.ok || verification.status !== "success") {
                        throw new Error(verification.message || "Payment verification failed.");
                    }
                    window.location.reload();
                } catch (error) {
                    statusBox.textContent = error.message;
                    button.disabled = false;
                }
            },
            modal: {ondismiss: () => { button.disabled = false; }}
        });
        checkout.open();
    } catch (error) {
        statusBox.textContent = error.message;
        button.disabled = false;
    }
}
</script>
</body>
</html>